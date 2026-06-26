// HFTASK-0082 (ADR-0019 D2): the rusty-idd toolkit is a SEPARATE co-located project, never on
// the kernel trust path. Its error-handling hardening (unwrap/expect/panic = deny) is the
// tracked follow-up HFTASK-0082; until then it opts out of the workspace deny lints so the
// kernel hardening (HFTASK-0080) is not blocked on the toolkit's ~577 sites.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
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
