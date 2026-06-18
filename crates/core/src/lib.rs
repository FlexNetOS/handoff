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
