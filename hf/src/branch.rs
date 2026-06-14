//! `branch` — the branch/remote policy engine (HFTASK-0008, ADR-0001 §3).
//!
//! Resolves the clone-vs-fork model and the base/trunk branches from `policy.toml`'s
//! `[remote]` section, and centralizes the branch enforcement rules so every verb
//! (`hf ship`, `hf session`) decides the same way instead of hardcoding `"master"`:
//!
//! - **branch off `origin/<base>` after fetch only** — `base_ref()` is the one legal
//!   start point (the fetch + fast-forward check stays in `session`).
//! - **never push the trunk directly** — `guard_direct_trunk_push()` refuses; work lands
//!   via PR onto the trunk.
//! - **keep develop and trunk in sync** (`develop_mirrors_trunk`) — `should_sync_develop_trunk()`.
//! - **fork is deferred** — recognized but `ensure_supported()` errors; clone is the only
//!   implemented model today (fork model deferred behind `remote.model = "fork"`).

use crate::policy::Remote;

/// The remote topology the loop runs under. `Clone` (one repo, branches) is implemented;
/// `Fork` (contributor fork → upstream PR) is recognized but deferred (ADR-0001 §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteModel {
    Clone,
    Fork,
}

impl RemoteModel {
    /// Parse the `remote.model` string; unknown values are an error (fail-closed) rather
    /// than a silent default, so a typo can't quietly change the topology.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "clone" => Ok(Self::Clone),
            "fork" => Ok(Self::Fork),
            other => Err(format!(
                "unknown remote.model '{other}' (expected 'clone' or 'fork')"
            )),
        }
    }
}

/// Resolved branch/remote policy — the single source of truth for branch decisions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchPolicy {
    pub model: RemoteModel,
    pub origin: String,
    /// Sessions/worktrees branch from `origin/<base>` (e.g. `develop`).
    pub base: String,
    /// The protected trunk PRs target and that is never pushed directly (e.g. `master`).
    pub trunk: String,
    /// Whether `develop` and the trunk are kept in lockstep.
    pub develop_mirrors_trunk: bool,
}

impl BranchPolicy {
    /// Resolve from the `[remote]` config. Errors only on an unknown model string.
    pub fn resolve(r: &Remote) -> Result<Self, String> {
        Ok(Self {
            model: RemoteModel::parse(&r.model)?,
            origin: r.origin.clone(),
            base: r.base_branch.clone(),
            trunk: r.trunk_branch.clone(),
            develop_mirrors_trunk: r.develop_mirrors_trunk,
        })
    }

    /// `origin/<base>` — the only legal branch-off point (after `git fetch`).
    pub fn base_ref(&self) -> String {
        format!("origin/{}", self.base)
    }

    /// True if `branch` is the protected trunk (or a common trunk alias).
    pub fn is_trunk(&self, branch: &str) -> bool {
        branch == self.trunk || branch == "master" || branch == "main"
    }

    /// Refuse to push/ship directly onto the trunk — work lands via PR only.
    pub fn guard_direct_trunk_push(&self, target: &str) -> Result<(), String> {
        if self.is_trunk(target) {
            return Err(format!(
                "refusing to push trunk '{target}' directly — branch off {} and open a PR",
                self.base_ref()
            ));
        }
        Ok(())
    }

    /// Fork model is recognized but not yet implemented; clone is the only supported flow.
    /// Verbs that perform remote operations call this to fail closed under `fork`.
    pub fn ensure_supported(&self) -> Result<(), String> {
        match self.model {
            RemoteModel::Clone => Ok(()),
            RemoteModel::Fork => Err(
                "remote.model = 'fork' is deferred (ADR-0001 §3) — only the clone model is implemented"
                    .into(),
            ),
        }
    }

    /// Whether trunk and develop should be kept in sync after a merge (the
    /// `develop_mirrors_trunk` rule) — only meaningful under the clone model with a
    /// distinct base and trunk.
    pub fn should_sync_develop_trunk(&self) -> bool {
        self.develop_mirrors_trunk && self.model == RemoteModel::Clone && self.base != self.trunk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(model: &str, base: &str, trunk: &str, mirror: bool) -> Remote {
        Remote {
            model: model.into(),
            origin: "FlexNetOS/handoff".into(),
            base_branch: base.into(),
            trunk_branch: trunk.into(),
            develop_mirrors_trunk: mirror,
        }
    }

    #[test]
    fn model_parse_clone_fork_and_unknown() {
        assert_eq!(RemoteModel::parse("clone").unwrap(), RemoteModel::Clone);
        assert_eq!(RemoteModel::parse(" Fork ").unwrap(), RemoteModel::Fork);
        assert!(RemoteModel::parse("svn").is_err());
    }

    #[test]
    fn resolve_defaults_to_clone_develop_master() {
        let bp = BranchPolicy::resolve(&Remote::default()).unwrap();
        assert_eq!(bp.model, RemoteModel::Clone);
        assert_eq!(bp.base, "develop");
        assert_eq!(bp.trunk, "master");
        assert_eq!(bp.base_ref(), "origin/develop");
    }

    #[test]
    fn resolve_rejects_unknown_model() {
        assert!(BranchPolicy::resolve(&remote("hg", "develop", "master", true)).is_err());
    }

    #[test]
    fn is_trunk_matches_trunk_and_aliases() {
        let bp = BranchPolicy::resolve(&remote("clone", "develop", "master", true)).unwrap();
        assert!(bp.is_trunk("master"));
        assert!(bp.is_trunk("main")); // alias guard
        assert!(!bp.is_trunk("develop"));
        assert!(!bp.is_trunk("handoff-HFTASK-0008-x"));
    }

    #[test]
    fn guard_refuses_direct_trunk_push_allows_feature() {
        let bp = BranchPolicy::resolve(&Remote::default()).unwrap();
        assert!(bp.guard_direct_trunk_push("master").is_err());
        assert!(bp.guard_direct_trunk_push("main").is_err());
        assert!(bp.guard_direct_trunk_push("handoff-HFTASK-0008").is_ok());
    }

    #[test]
    fn fork_model_is_deferred() {
        let clone = BranchPolicy::resolve(&Remote::default()).unwrap();
        assert!(clone.ensure_supported().is_ok());
        let fork = BranchPolicy::resolve(&remote("fork", "develop", "master", true)).unwrap();
        assert!(fork.ensure_supported().is_err());
    }

    #[test]
    fn develop_trunk_sync_only_under_clone_with_distinct_branches() {
        // clone + mirror + distinct base/trunk → sync.
        assert!(
            BranchPolicy::resolve(&remote("clone", "develop", "master", true))
                .unwrap()
                .should_sync_develop_trunk()
        );
        // mirror off → no sync.
        assert!(
            !BranchPolicy::resolve(&remote("clone", "develop", "master", false))
                .unwrap()
                .should_sync_develop_trunk()
        );
        // fork → no sync (deferred topology).
        assert!(
            !BranchPolicy::resolve(&remote("fork", "develop", "master", true))
                .unwrap()
                .should_sync_develop_trunk()
        );
        // base == trunk (trunk-based) → nothing to sync.
        assert!(
            !BranchPolicy::resolve(&remote("clone", "master", "master", true))
                .unwrap()
                .should_sync_develop_trunk()
        );
    }
}
