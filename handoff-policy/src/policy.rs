//! `.handoff/policy.toml` loader — `handoff.policy.v1` (ADR-0001 §1, HFTASK-0007).
//!
//! The loop's remote/loop/merge/preflight/sync configuration. Every section and key
//! is optional: a missing `policy.toml`, a missing section, or a missing key all fall
//! back to the compiled defaults below, so the kernel runs unconfigured (CI, fresh
//! clone) with the same behavior the ADR specifies.

use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub remote: Remote,
    #[serde(rename = "loop")]
    pub loop_cfg: Loop,
    pub merge: Merge,
    pub preflight: Preflight,
    pub sync: Sync,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Remote {
    pub model: String,
    pub origin: String,
    pub base_branch: String,
    pub trunk_branch: String,
    pub develop_mirrors_trunk: bool,
}
impl Default for Remote {
    fn default() -> Self {
        Self {
            model: "clone".into(),
            origin: "FlexNetOS/handoff".into(),
            base_branch: "develop".into(),
            trunk_branch: "master".into(),
            develop_mirrors_trunk: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Loop {
    /// Upper safety bound on tasks per cycle. With `wrap_strategy = "context"` (ADR-0018 D3) the
    /// loop wraps on the context budget first; `cycle_flush` only caps a runaway cycle.
    pub cycle_flush: u32,
    pub batch_checkout: bool,
    pub worktree_prefix: String,
    /// ADR-0018 D3: how the loop decides when to checkpoint → handoff.
    /// `"context"` = wrap when ~`context_budget_pct`% of the context window is consumed (default);
    /// `"tasks"` = legacy fixed `cycle_flush` count. Unknown values fall back to `"context"`.
    pub wrap_strategy: String,
    /// ADR-0018 D3: target fraction (percent) of the context window to consume before wrapping.
    /// The kernel exposes this policy; the loop-skill layer (handoff-loop / session-relay-wrap-up)
    /// reads the running token budget and triggers the wrap at the threshold.
    pub context_budget_pct: u8,
}
impl Default for Loop {
    fn default() -> Self {
        Self {
            cycle_flush: 4,
            batch_checkout: true,
            worktree_prefix: "handoff-".into(),
            wrap_strategy: "context".into(),
            context_budget_pct: 50,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Merge {
    pub require_review: bool,
    pub reviewer: String,
    pub auto_merge: String,
    pub permission_gate: bool,
    /// GitHub check-run / workflow job names that branch protection must require.
    ///
    /// ADR-0018 D8 grounds the autonomous gatekeeper as a required status check,
    /// never as a bot approval (gh-aw guardrail).
    pub required_status_checks: Vec<String>,
    /// Paths/prefixes that block automatic review/merge unless explicitly cleared.
    /// A file matches if it equals a pattern or starts with a pattern.
    /// Directory patterns should end in '/' to avoid false positives.
    pub protected_files: Vec<String>,
}
impl Default for Merge {
    fn default() -> Self {
        Self {
            require_review: true,
            reviewer: "cloud_ultra".into(),
            auto_merge: "on_approve".into(),
            permission_gate: true,
            required_status_checks: vec![
                "Test (ubuntu-latest)".into(),
                "Test (macos-latest)".into(),
                "Clippy".into(),
                "Format".into(),
                "AI Gatekeeper".into(),
            ],
            protected_files: vec![
                ".github/".into(),
                ".handoff/policy.toml".into(),
                "docs/adr-".into(),
                "schemas/".into(),
                "Cargo.lock".into(),
                "Cargo.toml".into(),
                ".agent/".into(),
                ".claude/rules/".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Preflight {
    pub require_clean_tree: bool,
    pub require_synced_base: bool,
    pub refuse_legacy_weave: bool,
}
impl Default for Preflight {
    fn default() -> Self {
        Self {
            require_clean_tree: true,
            require_synced_base: true,
            refuse_legacy_weave: true,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Sync {
    pub kb_enabled: bool,
    pub kb_slugs: Vec<String>,
    pub meta_register: bool,
}
impl Default for Sync {
    fn default() -> Self {
        Self {
            kb_enabled: true,
            kb_slugs: vec![
                "context/overridable/active".into(),
                "context/overridable/progress".into(),
            ],
            meta_register: true,
        }
    }
}

impl Merge {
    /// Return the subset of `files` that match the protected-files denylist.
    /// A file matches a pattern if it equals the pattern or starts with the pattern.
    /// Directory patterns should end in '/' to avoid false positives.
    pub fn protected_hits(&self, files: &[String]) -> Vec<String> {
        files
            .iter()
            .filter(|f| {
                self.protected_files.iter().any(|pat| {
                    let file = f.as_str();
                    let pat = pat.as_str();
                    file == pat || file.starts_with(pat)
                })
            })
            .cloned()
            .collect()
    }
}

impl Policy {
    /// Load `<hf_dir>/policy.toml`, falling back to defaults on absence or parse error
    /// (a malformed policy must never wall the kernel — it warns and uses defaults).
    pub fn load(hf_dir: &Path) -> Self {
        let path = hf_dir.join("policy.toml");
        match std::fs::read_to_string(&path) {
            Ok(s) => match toml::from_str::<Policy>(&s) {
                Ok(p) => p,
                Err(e) => {
                    eprintln!("hf: {} parse error ({e}); using defaults", path.display());
                    Policy::default()
                }
            },
            Err(_) => Policy::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_adr() {
        let p = Policy::default();
        assert_eq!(p.remote.base_branch, "develop");
        assert_eq!(p.remote.trunk_branch, "master");
        assert_eq!(p.loop_cfg.cycle_flush, 4);
        assert_eq!(p.loop_cfg.worktree_prefix, "handoff-");
        // ADR-0018 D3: context-budget wrap is the default (cycle_flush is now a safety bound).
        assert_eq!(p.loop_cfg.context_budget_pct, 50);
        assert_eq!(p.loop_cfg.wrap_strategy, "context");
        assert!(p.preflight.require_clean_tree);
        assert!(p.sync.kb_enabled);
        assert_eq!(p.sync.kb_slugs.len(), 2);
        assert!(
            p.merge
                .required_status_checks
                .contains(&"AI Gatekeeper".to_string())
        );
    }

    #[test]
    fn wrap_strategy_round_trips_for_the_tasks_opt_out() {
        // ADR-0018 D3: a consumer wraps on context unless wrap_strategy is exactly "tasks"
        // (the loop-skill layer reads this field; "unknown" stays on the new default).
        let p: Policy = toml::from_str("[loop]\nwrap_strategy = \"tasks\"\n").unwrap();
        assert_eq!(p.loop_cfg.wrap_strategy, "tasks");
        let typo: Policy = toml::from_str("[loop]\nwrap_strategy = \"contxt\"\n").unwrap();
        assert_ne!(typo.loop_cfg.wrap_strategy, "tasks"); // any non-"tasks" → context default
    }

    #[test]
    fn partial_toml_keeps_defaults_for_missing_keys() {
        // Only override one key; everything else must fall back to defaults.
        let toml = r#"
            [loop]
            cycle_flush = 7
        "#;
        let p: Policy = toml::from_str(toml).unwrap();
        assert_eq!(p.loop_cfg.cycle_flush, 7);
        assert_eq!(p.loop_cfg.worktree_prefix, "handoff-"); // default preserved
        assert_eq!(p.loop_cfg.context_budget_pct, 50); // default preserved
        assert_eq!(p.loop_cfg.wrap_strategy, "context"); // default strategy preserved
        assert_eq!(p.remote.base_branch, "develop"); // missing section → default
    }

    #[test]
    fn loop_keyword_section_parses() {
        // `loop` is a Rust keyword; the serde rename must accept the TOML section name.
        let toml = "[loop]\nworktree_prefix = \"x-\"\n";
        let p: Policy = toml::from_str(toml).unwrap();
        assert_eq!(p.loop_cfg.worktree_prefix, "x-");
    }

    #[test]
    fn merge_default_protected_files_is_non_empty() {
        let p = Policy::default();
        assert!(!p.merge.protected_files.is_empty());
        assert!(p.merge.protected_files.contains(&".github/".into()));
        assert!(
            p.merge
                .protected_files
                .contains(&".handoff/policy.toml".into())
        );
    }

    #[test]
    fn protected_files_prefix_and_exact_matching() {
        let p = Policy::default();
        let hits = p.merge.protected_hits(&[
            "src/main.rs".into(),
            ".github/workflows/ci.yml".into(),
            ".handoff/policy.toml".into(),
            "docs/adr-0001-keystone.md".into(),
            "docs/README.md".into(),
            "Cargo.lock".into(),
        ]);
        assert_eq!(hits.len(), 4);
        assert!(hits.contains(&".github/workflows/ci.yml".into()));
        assert!(hits.contains(&".handoff/policy.toml".into()));
        assert!(hits.contains(&"docs/adr-0001-keystone.md".into()));
        assert!(hits.contains(&"Cargo.lock".into()));
    }

    #[test]
    fn protected_files_configurable_via_toml() {
        let toml = r#"
            [merge]
            protected_files = ["SECRET", "vault/"]
        "#;
        let p: Policy = toml::from_str(toml).unwrap();
        let hits =
            p.merge
                .protected_hits(&["SECRET".into(), "vault/key.pem".into(), "src/lib.rs".into()]);
        assert_eq!(hits.len(), 2);
    }
}
