// HFTASK-0082 (ADR-0019 D5 #3): the rusty-idd toolkit now enforces the same error-handling deny
// lints (unwrap/expect/panic) in PRODUCTION as the kernel; they are allowed only under test
// (tests assert). The toolkit's production code already propagated errors, so the hardening was
// a clean flip — no bare production unwrap remained.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Intent Driven Development (IDD)
//!
//! A dependency-light, Rust-native toolkit for turning two related repositories
//! into a controlled AI-assisted unification workflow. The package intentionally
//! avoids network calls and provider-specific SDKs; GitHub/Copilot/OpenHands/
//! Cline/Aider-style agents can consume the generated markdown and JSON contracts
//! through normal issue/PR workflows.

pub mod cli;
pub mod env_contract;
pub mod fs_utils;
pub mod manifest;
pub mod model;
pub mod planner;
pub mod scanner;
pub mod templates;
pub mod validation;

pub fn run_from_env() -> Result<(), String> {
    cli::run(std::env::args())
}
