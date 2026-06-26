// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! handoff-policy — the branch/remote/merge/loop policy engine (ADR-0001 §3).
//!
//! Peeled out of the `hf` monolith as part of the 12-crate decomposition (ADR-0019 D5 #4,
//! HFTASK-0081), alongside `handoff-core`. Pure configuration logic with no `hf`-internal
//! dependencies: `policy` parses `.handoff/policy.toml` (remote model, loop budget, merge gates,
//! preflight, sync) and `branch` resolves the branch/remote questions (clone vs fork, base/trunk
//! refs, direct-trunk-push guard). `hf` re-exports both modules so existing `crate::policy::…` /
//! `crate::branch::…` paths stay valid (behavior-preserving move).

pub mod branch;
pub mod policy;
