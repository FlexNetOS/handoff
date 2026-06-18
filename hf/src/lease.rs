//! Weave lease bridge (WL-024): turns `hf claim` into a *mesh-coordinated* claim.
//!
//! A claim is no longer just a local ledger event — before recording it, `hf`
//! reserves an advisory lease on the task via the `weave` CLI
//! (`weave lease reserve --resource <r> --ttl <n> --note <s>`). If another peer
//! already holds the lease, the claim is refused; if the same peer re-claims,
//! weave extends the lease (this is the "heartbeat"); `release` frees it.
//!
//! Degrades gracefully: when `weave` is absent or too old to know `lease`, the
//! caller falls back to a ledger-only claim so the kernel still works offline
//! (CI, air-gapped). No shell is ever used — external programs are spawned with
//! an explicit argv, mirroring weave's own no-shell invariant.

use std::process::Command;

/// Outcome of a lease reservation attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reserve {
    /// We now hold the lease (fresh reservation or same-holder extension/heartbeat).
    Acquired,
    /// Another peer holds an active lease — the claim must be refused. Carries the
    /// raw reason reported by weave.
    Conflict(String),
    /// `weave` is unavailable or too old to support leases — fall back to ledger-only.
    Unsupported,
}

/// The lease resource key for claiming a task. Deliberately slash-free so weave's
/// path-hierarchy conflict detection reduces to exact-match — i.e. one holder per
/// task id, which is exactly "who owns this claim".
pub fn claim_resource(task_id: &str) -> String {
    format!("handoff:claim:{task_id}")
}

/// Pure classifier for a `weave lease reserve` invocation, split out so the policy
/// is unit-testable without spawning a process.
pub fn parse_reserve(success: bool, stdout: &str, stderr: &str) -> Reserve {
    if success {
        return Reserve::Acquired;
    }
    // Old/absent weave: clap emits an "unrecognized subcommand"/usage error on stderr.
    let se = stderr.to_ascii_lowercase();
    if se.contains("unrecognized subcommand") || se.contains("usage:") || se.contains("error:") {
        return Reserve::Unsupported;
    }
    // Genuine reserve failure (cross-holder conflict or validation): weave prints
    // "failed: <reason>" on stdout and exits 1.
    if let Some(reason) = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("failed:"))
    {
        return Reserve::Conflict(reason.trim().to_string());
    }
    // Unknown failure shape: prefer liveness over a hard wall — fall back to ledger-only.
    Reserve::Unsupported
}

/// What `hf claim` should do given a reservation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimGate {
    /// Lease held by us — record the claim normally.
    Proceed,
    /// No mesh available — record the claim ledger-only (degraded, with a warning).
    ProceedDegraded,
    /// Another peer owns the claim — refuse; do not touch the ledger. Carries the reason.
    Refuse(String),
}

/// Pure claim policy: map a reservation outcome to a gate decision.
pub fn gate(outcome: Reserve) -> ClaimGate {
    match outcome {
        Reserve::Acquired => ClaimGate::Proceed,
        Reserve::Unsupported => ClaimGate::ProceedDegraded,
        Reserve::Conflict(reason) => ClaimGate::Refuse(reason),
    }
}

// --- HFTASK-0048: in-ledger lease holder identity + on-disk `.handoff/locks/*.lock` mirror ---

/// This kernel's lease holder identity. Stable by default (so a single agent's repeated
/// `hf claim`/heartbeat doesn't self-conflict): `HF_LEASE_HOLDER` if set, else the hostname.
/// **Parallel agents on one host MUST set `HF_LEASE_HOLDER`** (e.g. the grit worktree/session
/// id) so the in-ledger lease gives them real mutual exclusion.
pub fn local_holder() -> String {
    std::env::var("HF_LEASE_HOLDER")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var("HOSTNAME")
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .unwrap_or_else(|| "localhost".to_string())
}

/// `.handoff/locks/<sanitized-resource>.lock` — the on-disk advisory mirror of an in-ledger
/// lease (the ledger is authoritative; this file is for cross-tool visibility). Colons/slashes
/// in the resource are flattened so the name is a single safe filename.
pub fn lockfile_path(resource: &str) -> std::path::PathBuf {
    let safe: String = resource
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::path::Path::new(".handoff")
        .join("locks")
        .join(format!("{safe}.lock"))
}

/// Write the advisory lockfile mirror (best-effort; failure never blocks the ledger lease).
pub fn write_lockfile(resource: &str, holder: &str, ttl_secs: u64, acquired_ns: u64) {
    let path = lockfile_path(resource);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let body = format!(
        "{{\"resource\":\"{resource}\",\"holder\":\"{holder}\",\"ttl_secs\":{ttl_secs},\"acquired_ns\":{acquired_ns}}}\n"
    );
    let _ = std::fs::write(&path, body);
}

/// Remove the advisory lockfile mirror on release (best-effort).
pub fn remove_lockfile(resource: &str) {
    let _ = std::fs::remove_file(lockfile_path(resource));
}

/// A source of advisory leases. Abstracted so `hf claim` can be tested against an
/// in-memory fake instead of a live `weave` mesh.
pub trait Leaser {
    /// Reserve `resource` for `ttl` seconds with an optional `note`.
    fn reserve(&self, resource: &str, ttl: u64, note: &str) -> Reserve;
    /// Release a lease we hold on `resource`. Returns whether weave confirmed it.
    fn release(&self, resource: &str) -> bool;
}

/// Real bridge: shells out to the `weave` binary (overridable via `HF_WEAVE_BIN`).
pub struct WeaveCli {
    pub bin: String,
}

impl WeaveCli {
    pub fn from_env() -> Self {
        Self {
            bin: std::env::var("HF_WEAVE_BIN").unwrap_or_else(|_| "weave".into()),
        }
    }
}

impl Leaser for WeaveCli {
    fn reserve(&self, resource: &str, ttl: u64, note: &str) -> Reserve {
        let out = Command::new(&self.bin)
            .args([
                "lease",
                "reserve",
                "--resource",
                resource,
                "--ttl",
                &ttl.to_string(),
                "--note",
                note,
            ])
            .output();
        match out {
            Ok(o) => parse_reserve(
                o.status.success(),
                &String::from_utf8_lossy(&o.stdout),
                &String::from_utf8_lossy(&o.stderr),
            ),
            // weave not installed / not on PATH: degrade, don't wall.
            Err(_) => Reserve::Unsupported,
        }
    }

    fn release(&self, resource: &str) -> bool {
        Command::new(&self.bin)
            .args(["lease", "release", "--resource", resource])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claim_resource_is_slash_free_and_namespaced() {
        assert_eq!(claim_resource("HFTASK-0002"), "handoff:claim:HFTASK-0002");
        assert!(!claim_resource("HFTASK-0002").contains('/'));
    }

    #[test]
    fn parse_reserve_success_is_acquired() {
        assert_eq!(
            parse_reserve(true, "leased handoff:claim:HFTASK-0002 (expires ...)", ""),
            Reserve::Acquired
        );
    }

    #[test]
    fn parse_reserve_failed_is_conflict_with_reason() {
        assert_eq!(
            parse_reserve(false, "failed: resource already leased by peer-x", ""),
            Reserve::Conflict("resource already leased by peer-x".to_string())
        );
    }

    #[test]
    fn parse_reserve_old_weave_is_unsupported() {
        assert_eq!(
            parse_reserve(false, "", "error: unrecognized subcommand 'lease'"),
            Reserve::Unsupported
        );
    }

    #[test]
    fn parse_reserve_unknown_failure_falls_back() {
        assert_eq!(parse_reserve(false, "weird", ""), Reserve::Unsupported);
    }

    #[test]
    fn gate_maps_outcomes_to_claim_policy() {
        assert_eq!(gate(Reserve::Acquired), ClaimGate::Proceed);
        assert_eq!(gate(Reserve::Unsupported), ClaimGate::ProceedDegraded);
        assert_eq!(
            gate(Reserve::Conflict("held by peer-x".into())),
            ClaimGate::Refuse("held by peer-x".into())
        );
    }

    /// In-memory fake mesh: first holder wins; a different holder is refused; the
    /// same holder may re-reserve (heartbeat); release frees the resource.
    struct FakeMesh {
        me: &'static str,
        held_by: std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, String>>>,
    }
    impl Leaser for FakeMesh {
        fn reserve(&self, resource: &str, _ttl: u64, _note: &str) -> Reserve {
            let mut map = self.held_by.borrow_mut();
            match map.get(resource) {
                Some(h) if h == self.me => Reserve::Acquired, // heartbeat / extend
                Some(h) => Reserve::Conflict(format!("held by {h}")),
                None => {
                    map.insert(resource.to_string(), self.me.to_string());
                    Reserve::Acquired
                }
            }
        }
        fn release(&self, resource: &str) -> bool {
            self.held_by.borrow_mut().remove(resource).is_some()
        }
    }

    #[test]
    fn fake_mesh_coordinates_claims() {
        let shared = std::rc::Rc::new(std::cell::RefCell::new(std::collections::HashMap::new()));
        let res = claim_resource("HFTASK-0002");

        // We claim it: acquired -> Proceed.
        let me = FakeMesh {
            me: "me",
            held_by: shared.clone(),
        };
        assert_eq!(gate(me.reserve(&res, 3600, "")), ClaimGate::Proceed);
        // Heartbeat (same holder re-reserves) stays Proceed.
        assert_eq!(gate(me.reserve(&res, 3600, "")), ClaimGate::Proceed);

        // A different peer is refused while we hold it.
        let other = FakeMesh {
            me: "other",
            held_by: shared.clone(),
        };
        assert_eq!(
            gate(other.reserve(&res, 3600, "")),
            ClaimGate::Refuse("held by me".into())
        );

        // After we release, the other peer can claim.
        assert!(me.release(&res));
        assert_eq!(gate(other.reserve(&res, 3600, "")), ClaimGate::Proceed);
    }
}
