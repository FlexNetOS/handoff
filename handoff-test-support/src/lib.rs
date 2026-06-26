//! Shared test-only helpers (HFTASK-0029; HFTASK-0083: lifted into its own crate so the peeled
//! feature crates — handoff-route, handoff-gatekeeper, … — serialize their cwd-mutating tests the
//! same way hf did).
//!
//! Several `hf` operations resolve `.handoff/` relative to the process cwd (routing, seed, ship
//! card-staging). Tests that exercise those paths must mutate the global process cwd, which races
//! across cargo's parallel test threads. A single per-test-binary mutex serializes every
//! cwd-mutating test within that binary so they can't interleave. (Each crate's test binary is a
//! separate process with its own cwd, so a per-binary mutex is exactly the right scope.)

/// Serialize cwd-mutating tests behind ONE process-wide mutex. Held for the duration of the
/// returned guard; poisoning is ignored (a panicking test still releases the cwd).
pub fn cwd_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
