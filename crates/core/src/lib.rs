// HFTASK-0082 (ADR-0019 D2): the rusty-idd toolkit is a SEPARATE co-located project, never on
// the kernel trust path. Its error-handling hardening (unwrap/expect/panic = deny) is the
// tracked follow-up HFTASK-0082; until then it opts out of the workspace deny lints so the
// kernel hardening (HFTASK-0080) is not blocked on the toolkit's ~577 sites.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
