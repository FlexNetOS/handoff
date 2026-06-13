//! Shared test-only helpers (HFTASK-0029).
//!
//! Several `hf` operations resolve `.handoff/` relative to the process cwd (routing,
//! seed, ship card-staging). Tests that exercise those paths must mutate the global
//! process cwd, which races across cargo's parallel test threads. A single crate-wide
//! mutex serializes every cwd-mutating test (across `route`, `main`, etc.) so they can't
//! interleave.

/// Serialize cwd-mutating tests behind ONE process-wide mutex. Held for the duration of
/// the returned guard; poisoning is ignored (a panicking test still releases the cwd).
pub(crate) fn cwd_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}
