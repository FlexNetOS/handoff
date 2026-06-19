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
