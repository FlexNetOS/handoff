//! `rusty-idd run` — headless task runner over `openspec-runner` (no ratatui).
//!
//! Builds a [`TuiConfig`] (default, or loaded from `openspec/tui-config.yaml`,
//! with optional `--command` / `--prompt` overrides), starts the
//! implementation (or apply) worker, drains its `ImplUpdate` channel, and
//! prints progress lines until `Finished` or `Stalled`.

use std::path::Path;

use clap::Args;
use rusty_idd_runner::config::{TuiConfig, CONFIG_PATH};
use rusty_idd_runner::runner::{start_apply, start_implementation, ImplUpdate};

#[derive(Args)]
pub struct RunArgs {
    /// The change name (a directory under `openspec/changes/`).
    pub change: String,
    /// Use apply mode (single `/opsx:apply` run) instead of per-task looping.
    #[arg(long)]
    pub apply: bool,
    /// Override the command template (`{prompt}` placeholder).
    #[arg(long)]
    pub command: Option<String>,
    /// Override the per-task prompt template (`{name}` placeholder).
    #[arg(long)]
    pub prompt: Option<String>,
}

/// Run a headless implementation/apply pass. Returns a process exit code.
pub fn run(args: RunArgs) -> i32 {
    let mut config = match TuiConfig::load_from(Path::new(CONFIG_PATH)) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rusty-idd: failed to load {CONFIG_PATH}: {e}");
            return 1;
        }
    };
    if let Some(command) = args.command {
        config.command = command;
    }
    if let Some(prompt) = args.prompt {
        config.prompt = prompt;
    }

    let state = if args.apply {
        println!("Applying change '{}'...", args.change);
        start_apply(&args.change, &config)
    } else {
        let state = start_implementation(&args.change, &config);
        println!(
            "Running change '{}' ({}/{} tasks complete)...",
            args.change, state.completed, state.total
        );
        state
    };

    // Drain the update channel until the worker signals completion.
    let mut exit = 0;
    for update in &state.receiver {
        match update {
            ImplUpdate::Progress { completed, total } => {
                println!("  progress: {completed}/{total} tasks complete");
            }
            ImplUpdate::Finished { success } => {
                if success {
                    println!("Finished: '{}' completed successfully.", args.change);
                } else {
                    eprintln!("Finished: '{}' did not complete successfully.", args.change);
                    exit = 1;
                }
                break;
            }
            ImplUpdate::Stalled => {
                eprintln!(
                    "Stalled: '{}' made no progress after repeated attempts.",
                    args.change
                );
                exit = 1;
                break;
            }
            ImplUpdate::Error(e) => {
                eprintln!("Error running change '{}': {}", args.change, e);
                exit = 1;
                break;
            }
        }
    }

    exit
}
