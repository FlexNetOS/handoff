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
    pub cycle_flush: u32,
    pub batch_checkout: bool,
    pub worktree_prefix: String,
}
impl Default for Loop {
    fn default() -> Self {
        Self {
            cycle_flush: 4,
            batch_checkout: true,
            worktree_prefix: "handoff-".into(),
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
}
impl Default for Merge {
    fn default() -> Self {
        Self {
            require_review: true,
            reviewer: "cloud_ultra".into(),
            auto_merge: "on_approve".into(),
            permission_gate: true,
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
        assert!(p.preflight.require_clean_tree);
        assert!(p.sync.kb_enabled);
        assert_eq!(p.sync.kb_slugs.len(), 2);
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
        assert_eq!(p.remote.base_branch, "develop"); // missing section → default
    }

    #[test]
    fn loop_keyword_section_parses() {
        // `loop` is a Rust keyword; the serde rename must accept the TOML section name.
        let toml = "[loop]\nworktree_prefix = \"x-\"\n";
        let p: Policy = toml::from_str(toml).unwrap();
        assert_eq!(p.loop_cfg.worktree_prefix, "x-");
    }
}
