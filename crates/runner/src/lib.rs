// HFTASK-0082 (ADR-0019 D5 #3): the rusty-idd toolkit now enforces the same error-handling deny
// lints (unwrap/expect/panic) in PRODUCTION as the kernel; they are allowed only under test
// (tests assert). The toolkit's production code already propagated errors, so the hardening was
// a clean flip — no bare production unwrap remained.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! rusty-idd-runner — the non-UI execution layer extracted from the TUI.
//!
//! Holds the task-execution engine (`runner`: spawn an agent CLI, stream
//! progress, stall detection, batch ordering), the OpenSpec data layer
//! (`data`: parse `tasks.md`, list changes), and the run configuration
//! (`config`: `TuiConfig`). Both `rusty-idd-tui` and `rusty-idd-cli` consume
//! these — the CLI's `rusty-idd run` drives task execution without ratatui.

pub mod config;
pub mod data;
pub mod runner;
