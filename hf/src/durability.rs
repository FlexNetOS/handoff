//! HFTASK-0058 / ADR-0016: canonical `.handoff` durability policy — the single,
//! kernel-owned definition of which `.handoff` subpaths are **durable** (source of
//! truth, MUST be committed) vs **regenerable** (rebuilt by `hf`, MUST be gitignored),
//! plus the *swallow-guard* that fail-closed detects when a durable path is being
//! silently ignored.
//!
//! ## The bug this prevents (found 2026-06-21 verifying the rust-port harness)
//!
//! `.handoff/` is the continuity source-of-truth everywhere the kernel is used, but the
//! ignore policy was *implicit* — every consumer hand-rolled its own `.gitignore` and got
//! it wrong. The sharpest failure: a **dir-form** ignore (`.handoff/` or `.claude/`, a bare
//! directory with no `/*` and no negation) makes Git refuse to re-include ANYTHING beneath
//! it — `!`-exceptions cannot rescue a path whose parent directory is excluded. So a durable
//! `.handoff/tasks/*.json` or a loop `*.md` ledger is **silently swallowed**: `git add`
//! stages zero, continuity dies, and nothing detects it. Only `git check-ignore` on a
//! representative durable path surfaces it — which is exactly what [`swallow_report`] does.
//!
//! ## The taxonomy (ADR-0016, REVISED by ADR-0018 D1)
//!
//! ADR-0018 D1 (full-auto: commit ALL dotfiles, incl. rendered views) **moves the rendered
//! views into the durable set** and settles the ledger as a *text* export (HFTASK-0067):
//!
//! - **DURABLE** (commit; un-ignored): [`DURABLE_PROBES`] — `tasks/`, `decisions/`,
//!   `context/` (capsule), loop `*.md` ledgers, `hooks/`, `policy.toml`, `README.md`,
//!   **`ledger.events.jsonl`** (the committed continuity truth — the deterministic JSONL
//!   export, ADR-0018 D1 / increment 1), and the **rendered views** `packets/`, `active.md`,
//!   `deliveries/` (now committed so a fresh clone / observer sees rendered state).
//! - **REGENERABLE** (ignore; local rebuild cache): [`REGENERABLE_PROBES`] — the binary
//!   ledger cache `*.db*` / `*.rvf*` (re-derived from `ledger.events.jsonl` via `hf import`),
//!   the ephemeral `workspaces/` + `locks/`, and the out-of-tree migration artifacts
//!   `*.sqlite.bak` / `*.redb.tmp`.
//!
//! Why the binary stays ignored while "commit ALL dotfiles" holds: a churning binary DB is
//! never committable best-practice (merge hell, bloat) — so the *durable text* form (the
//! JSONL export) is the committed artifact and the binary is its local cache. This is the
//! "deterministic text export beside it" branch HFTASK-0067 authorizes.
//!
//! The kernel SHIPS [`CANONICAL_GITIGNORE_FRAGMENT`] (contents-form only) so consumers
//! inherit the policy instead of hand-rolling it, and [`repair_gitignore`] writes/repairs it
//! — and now also **strips the retired derived-view ignores** ([`RETIRED_VIEW_IGNORES`]) so a
//! repo migrating off the old policy re-includes its rendered views.

use std::path::Path;
use std::process::Command;

/// Representative **durable** `.handoff` paths that MUST remain committable (never ignored).
/// These are probe paths for `git check-ignore` — they need not exist on disk. If any of
/// these is reported ignored, a durable swallow is in effect.
pub const DURABLE_PROBES: &[&str] = &[
    ".handoff/tasks/HFTASK-PROBE.task.json",
    ".handoff/decisions/adr-probe.md",
    ".handoff/context/capsule.json",
    ".handoff/loop/loop_state.md",
    ".handoff/hooks/probe.sh",
    ".handoff/policy.toml",
    ".handoff/README.md",
    // ADR-0018 D1: the committed continuity truth (the JSONL export — HFTASK-0067 increment 1)
    // and the rendered views are now DURABLE (committed), not regenerable-ignored.
    ".handoff/ledger.events.jsonl",
    ".handoff/packets/latest.md",
    ".handoff/active.md",
    ".handoff/deliveries/corr.delivery.json",
];

/// Representative **regenerable** `.handoff` paths that MUST stay gitignored — the local rebuild
/// cache (re-derived from the committed `ledger.events.jsonl` via `hf import`), the ephemeral
/// runtime state, and the out-of-tree migration artifacts. Used to detect *under-ignoring*
/// regressions. ADR-0018 D1: the rendered views (`packets/`, `active.md`, `deliveries/`) left
/// this set — they are now committed.
pub const REGENERABLE_PROBES: &[&str] = &[
    ".handoff/ledger.db",
    ".handoff/ledger.db-wal",
    ".handoff/ledger.db-shm",
    ".handoff/ledger.db.rvf",
    ".handoff/ledger.db.rvf.lock",
    ".handoff/ledger.db.sqlite.bak",
    ".handoff/ledger.redb.tmp",
    ".handoff/workspaces/main/scratch",
    ".handoff/locks/task.lock",
];

/// ADR-0018 D1 migration: the derived-view `.gitignore` lines the old (pre-0067) policy shipped
/// that MUST now be stripped — the rendered views are durable/committed. [`repair_gitignore`]
/// removes any line whose normalized form matches one of these so a migrating repo re-includes
/// its views; [`swallow_report`] independently flags them as swallowed-durable. Normalized form:
/// trimmed, leading `**/` and surrounding `/` removed (matches [`normalize_ignore_line`]).
pub const RETIRED_VIEW_IGNORES: &[&str] = &[
    ".handoff/packets",
    ".handoff/active.md",
    ".handoff/deliveries",
];

/// Continuity directories that must NEVER be excluded in dir-form (a bare ignore of the whole
/// directory swallows durable children that negations cannot rescue).
const SWALLOW_DIRS: &[&str] = &[".handoff", ".claude"];

/// Marker substring identifying the kernel-shipped fragment in a consumer `.gitignore`
/// (idempotency). Deliberately ADR-suffix-free so it matches both the original
/// `(ADR-0016)` header and the ADR-0018-D1-revised `(ADR-0016 / ADR-0018 D1)` header — a repo
/// that adopted the old fragment is still recognized as having it (no duplicate append on migrate).
pub const FRAGMENT_MARKER: &str = "canonical .handoff durability policy";

/// The canonical, **contents-form** `.gitignore` fragment the kernel ships. Every rule targets
/// a specific regenerable subpath — there is deliberately no bare `.handoff/` / `.claude/`
/// line, because that form swallows durable state. Durable paths (`tasks/`, `decisions/`,
/// `context/`, loop `*.md`, `hooks/`, `policy.toml`, `README.md`) are intentionally absent
/// here so they stay committed.
pub const CANONICAL_GITIGNORE_FRAGMENT: &str = "\
# === handoff continuity kernel — canonical .handoff durability policy (ADR-0016 / ADR-0018 D1) ===
# REGENERABLE .handoff state — the LOCAL rebuild cache (the binary ledger is re-derived from the
# committed `.handoff/ledger.events.jsonl` via `hf import`) + ephemeral runtime + out-of-tree
# migration artifacts: never commit. The committed continuity truth is the JSONL export and the
# rendered views (packets/, active.md, deliveries/) — those are DURABLE and intentionally absent
# here so they stay committed (ADR-0018 D1).
# CONTENTS-FORM only — NEVER a bare `.handoff/` or `.claude/` ignore: Git cannot re-include a
# path past an excluded parent dir, so a dir-form ignore silently SWALLOWS durable ledgers
# (tasks/, decisions/, loop/*.md). Run `hf gitignore --check` (or `hf doctor`) to detect it.
.handoff/**/ledger.db
.handoff/**/*.db-wal
.handoff/**/*.db-shm
.handoff/**/*.rvf
.handoff/**/*.rvf.lock
.handoff/**/*.sqlite.bak
.handoff/**/*.redb.tmp
/.handoff/workspaces/
/.handoff/locks/
";

/// A single `.gitignore` line normalized for dir-form comparison: trimmed, comments/negations
/// dropped, and leading `**/` + surrounding `/` removed. Returns `None` for lines that cannot
/// be a dir-form swallow (blank, comment, or negation — negations re-include, they never swallow).
fn normalize_ignore_line(line: &str) -> Option<String> {
    let l = line.trim();
    if l.is_empty() || l.starts_with('#') || l.starts_with('!') {
        return None;
    }
    let l = l.strip_prefix("**/").unwrap_or(l);
    let l = l.trim_start_matches('/').trim_end_matches('/');
    if l.is_empty() {
        return None;
    }
    Some(l.to_string())
}

/// Scan `.gitignore` text and return the raw lines that exclude a whole continuity directory
/// in dir-form (`.handoff`, `/.handoff/`, `**/.handoff/`, `.claude/`, …). These are the
/// swallow culprits. Pure — testable without a git repo.
pub fn scan_dir_form_ignores(gitignore: &str) -> Vec<String> {
    gitignore
        .lines()
        .filter(|line| {
            normalize_ignore_line(line).is_some_and(|n| SWALLOW_DIRS.contains(&n.as_str()))
        })
        .map(|l| l.trim().to_string())
        .collect()
}

/// ADR-0018 D1 migration: scan `.gitignore` text for the retired derived-view ignore lines
/// (`packets/`, `active.md`, `deliveries/`) the old policy shipped — those views are now
/// committed, so these lines must be stripped. Returns the raw matching lines. Pure.
pub fn scan_retired_view_ignores(gitignore: &str) -> Vec<String> {
    gitignore
        .lines()
        .filter(|line| {
            normalize_ignore_line(line).is_some_and(|n| RETIRED_VIEW_IGNORES.contains(&n.as_str()))
        })
        .map(|l| l.trim().to_string())
        .collect()
}

/// Ask Git whether `path` is ignored in `repo`. `git check-ignore -q` exits 0 iff the path is
/// ignored, 1 if not, 128 on error (not a git repo) — so a non-repo reads as "not ignored".
fn path_is_ignored(repo: &Path, path: &str) -> bool {
    Command::new("git")
        .args(["-C", &repo.to_string_lossy(), "check-ignore", "-q", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The durability health of a repo's `.handoff` ignore policy.
#[derive(Debug, Default)]
pub struct SwallowReport {
    /// Raw `.gitignore` lines that exclude a whole continuity directory in dir-form.
    pub dir_form_ignores: Vec<String>,
    /// Durable probe paths that Git reports as IGNORED (the swallow — fatal).
    pub swallowed_durable: Vec<String>,
    /// Regenerable probe paths that are NOT ignored (under-ignoring regression — a warning).
    pub regenerable_unignored: Vec<String>,
}

impl SwallowReport {
    /// Healthy iff no durable path is swallowed and no dir-form continuity ignore exists.
    /// Under-ignoring regenerable state is reported (`regenerable_unignored`) but is a warning,
    /// not a swallow — it does not flip health, so the guard stays a strict superset of "the
    /// durable truth is committable".
    pub fn is_healthy(&self) -> bool {
        self.dir_form_ignores.is_empty() && self.swallowed_durable.is_empty()
    }
}

/// Run the swallow-guard over `repo`: detect dir-form continuity ignores, durable paths that
/// are being ignored, and regenerable paths that escaped the ignore set. The detection lives
/// in Git's own ignore engine (`git check-ignore`) — these bugs are invisible from reading
/// code and only surface by asking Git on a real tree.
pub fn swallow_report(repo: &Path) -> SwallowReport {
    let gitignore = std::fs::read_to_string(repo.join(".gitignore")).unwrap_or_default();
    SwallowReport {
        dir_form_ignores: scan_dir_form_ignores(&gitignore),
        swallowed_durable: DURABLE_PROBES
            .iter()
            .filter(|p| path_is_ignored(repo, p))
            .map(|p| p.to_string())
            .collect(),
        regenerable_unignored: REGENERABLE_PROBES
            .iter()
            .filter(|p| !path_is_ignored(repo, p))
            .map(|p| p.to_string())
            .collect(),
    }
}

/// What [`repair_gitignore`] changed.
#[derive(Debug, Default)]
pub struct RepairOutcome {
    /// Dir-form continuity-ignore lines that were removed (they swallow durable state).
    pub removed_dir_form: Vec<String>,
    /// ADR-0018 D1: retired derived-view ignore lines removed (`packets/`/`active.md`/`deliveries/`
    /// — those views are now committed).
    pub removed_retired_views: Vec<String>,
    /// Whether the canonical fragment was appended (false if it was already present).
    pub added_fragment: bool,
}

impl RepairOutcome {
    pub fn changed(&self) -> bool {
        self.added_fragment
            || !self.removed_dir_form.is_empty()
            || !self.removed_retired_views.is_empty()
    }
}

/// Write/repair `repo/.gitignore` to the canonical policy, idempotently and non-destructively
/// except for the two things that MUST go: (1) any dir-form `.handoff`/`.claude` exclude (the
/// swallow culprit — removing it re-includes durable state) and (2) the retired derived-view
/// ignores (`packets/`/`active.md`/`deliveries/` — now committed per ADR-0018 D1). It then
/// appends [`CANONICAL_GITIGNORE_FRAGMENT`] if the marker is absent. All other lines are
/// preserved verbatim.
pub fn repair_gitignore(repo: &Path) -> std::io::Result<RepairOutcome> {
    let path = repo.join(".gitignore");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let removed = scan_dir_form_ignores(&text);
    let removed_views = scan_retired_view_ignores(&text);

    let kept: Vec<&str> = text
        .lines()
        .filter(|line| {
            normalize_ignore_line(line).is_none_or(|n| {
                !SWALLOW_DIRS.contains(&n.as_str()) && !RETIRED_VIEW_IGNORES.contains(&n.as_str())
            })
        })
        .collect();
    let mut out = kept.join("\n");

    let added_fragment = !text.contains(FRAGMENT_MARKER);
    if added_fragment {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(CANONICAL_GITIGNORE_FRAGMENT);
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
    std::fs::write(&path, out)?;
    Ok(RepairOutcome {
        removed_dir_form: removed,
        removed_retired_views: removed_views,
        added_fragment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn scan_flags_dir_form_but_not_contents_form_or_negations() {
        let gi = "\
/target
.handoff/
/.claude/
**/.handoff/
.handoff/**/ledger.db
/.handoff/packets/
!.handoff/tasks/
.handoffx
";
        let flagged = scan_dir_form_ignores(gi);
        // Three dir-form continuity excludes: `.handoff/`, `/.claude/`, `**/.handoff/`.
        assert_eq!(flagged.len(), 3, "flagged: {flagged:?}");
        assert!(flagged.iter().any(|l| l == ".handoff/"));
        assert!(flagged.iter().any(|l| l == "/.claude/"));
        assert!(flagged.iter().any(|l| l == "**/.handoff/"));
        // Contents-form, negations, and look-alikes are NOT flagged.
        assert!(!flagged.iter().any(|l| l.contains("ledger.db")));
        assert!(!flagged.iter().any(|l| l.contains("packets")));
        assert!(!flagged.iter().any(|l| l.starts_with('!')));
        assert!(!flagged.iter().any(|l| l == ".handoffx"));
    }

    fn temp_repo() -> std::path::PathBuf {
        let repo = std::env::temp_dir().join(format!(
            "hf-durability-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(repo.join(".handoff")).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .args(["-C", repo.to_str().unwrap()])
                .args(args)
                .output()
                .unwrap()
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@example.com"]);
        git(&["config", "user.name", "t"]);
        repo
    }

    /// THE acceptance test: a dir-form `.handoff/` ignore swallows durable state, the guard
    /// fail-closed detects it, `repair_gitignore` fixes it, and afterwards the durable paths
    /// are committable while the regenerable set stays ignored (no regression).
    #[test]
    fn guard_detects_dir_form_swallow_and_repair_fixes_it() {
        let repo = temp_repo();

        // BUG STATE: a bare `.handoff/` ignore — the swallow.
        std::fs::write(repo.join(".gitignore"), "/target\n.handoff/\n").unwrap();
        let before = swallow_report(&repo);
        assert!(
            !before.is_healthy(),
            "dir-form .handoff/ must be reported unhealthy"
        );
        assert!(
            !before.dir_form_ignores.is_empty(),
            "the dir-form line must be flagged"
        );
        assert!(
            !before.swallowed_durable.is_empty(),
            "durable probes (tasks/decisions/loop) must read as swallowed under .handoff/"
        );

        // REPAIR: strip the dir-form line, append the canonical contents-form fragment.
        let outcome = repair_gitignore(&repo).unwrap();
        assert!(outcome.changed());
        assert!(outcome.added_fragment);
        assert_eq!(outcome.removed_dir_form, vec![".handoff/".to_string()]);

        // FIXED STATE: healthy, durable un-ignored, regenerable still ignored (no regression).
        let after = swallow_report(&repo);
        assert!(
            after.is_healthy(),
            "after repair the swallow is gone: {after:?}"
        );
        assert!(
            after.swallowed_durable.is_empty(),
            "no durable path is ignored after repair"
        );
        assert!(
            after.regenerable_unignored.is_empty(),
            "every regenerable probe is still ignored (no regression): {:?}",
            after.regenerable_unignored
        );
        // `/target` (a pre-existing unrelated line) is preserved.
        let gi = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(gi.contains("/target"));
        assert!(gi.contains(FRAGMENT_MARKER));

        // Idempotent: a second repair adds nothing.
        let again = repair_gitignore(&repo).unwrap();
        assert!(!again.added_fragment);
        assert!(again.removed_dir_form.is_empty());

        std::fs::remove_dir_all(&repo).ok();
    }

    /// ADR-0018 D1 migration: a repo carrying the OLD policy (the retired derived-view ignores
    /// `packets/`/`active.md`/`deliveries/`) is unhealthy — those views are now durable, so they
    /// read as swallowed — and `repair_gitignore` strips the retired lines so they re-include,
    /// while the binary cache stays ignored.
    #[test]
    fn migration_strips_retired_view_ignores_and_recommits_views() {
        let repo = temp_repo();

        // OLD-POLICY STATE: the pre-0067 contents-form fragment ignored the rendered views.
        std::fs::write(
            repo.join(".gitignore"),
            "/target\n.handoff/**/ledger.db\n/.handoff/packets/\n/.handoff/active.md\n/.handoff/deliveries/\n",
        )
        .unwrap();

        // The rendered views read as swallowed-durable (they are now durable, still ignored).
        let before = swallow_report(&repo);
        assert!(
            !before.is_healthy(),
            "old retired-view ignores must read unhealthy: {before:?}"
        );
        assert!(
            before
                .swallowed_durable
                .iter()
                .any(|p| p.contains("active.md")),
            "active.md must read as a swallowed durable view: {before:?}"
        );
        assert_eq!(
            scan_retired_view_ignores(&std::fs::read_to_string(repo.join(".gitignore")).unwrap())
                .len(),
            3,
            "all three retired-view ignore lines must be detected"
        );

        // REPAIR strips the retired-view lines (and appends the new canonical fragment).
        let outcome = repair_gitignore(&repo).unwrap();
        assert!(outcome.changed());
        assert_eq!(
            outcome.removed_retired_views.len(),
            3,
            "all three retired-view ignores removed: {outcome:?}"
        );

        // FIXED: views committable, binary cache still ignored, no regression.
        let after = swallow_report(&repo);
        assert!(after.is_healthy(), "after migration repair: {after:?}");
        assert!(after.swallowed_durable.is_empty());
        assert!(
            after.regenerable_unignored.is_empty(),
            "binary cache + ephemeral state still ignored: {:?}",
            after.regenerable_unignored
        );
        // The ledger guard (a non-view line) is preserved.
        let gi = std::fs::read_to_string(repo.join(".gitignore")).unwrap();
        assert!(gi.contains(".handoff/**/ledger.db"));
        std::fs::remove_dir_all(&repo).ok();
    }

    /// A from-scratch repo that adopts ONLY the canonical fragment is healthy: durable paths
    /// are committable and every regenerable path is ignored.
    #[test]
    fn canonical_fragment_alone_is_healthy_and_complete() {
        let repo = temp_repo();
        std::fs::write(repo.join(".gitignore"), CANONICAL_GITIGNORE_FRAGMENT).unwrap();
        let rep = swallow_report(&repo);
        assert!(rep.is_healthy(), "canonical fragment is healthy: {rep:?}");
        assert!(
            rep.regenerable_unignored.is_empty(),
            "fragment ignores the full regenerable set: {:?}",
            rep.regenerable_unignored
        );
        assert!(rep.swallowed_durable.is_empty());
        std::fs::remove_dir_all(&repo).ok();
    }
}
