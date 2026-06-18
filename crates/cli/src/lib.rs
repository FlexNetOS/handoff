//! `rusty-idd` — the unified CLI that wires the existing crates together behind
//! one clap binary:
//!
//! - **Core verbs** (`init scan plan task validate manifest github`) delegate
//!   *verbatim* to [`rusty_idd_core::cli::run`], giving automatic
//!   parity with the legacy `idd` binary (same code path).
//! - **`spec`** is a new CLI over [`rusty_idd_spec`] (validate / archive / show).
//! - **`run`** is a headless task runner over [`rusty_idd_runner`] (no ratatui).
//! - **`tui`** launches the interactive TUI via [`rusty_idd_tui::run`].
//!
//! Dependencies live at this crate (and the tui); the core crate stays zero-dep.

use clap::{Parser, Subcommand};

mod commands;

/// Top-level `rusty-idd` parser.
#[derive(Parser)]
#[command(
    name = "rusty-idd",
    version,
    about = "Unified intent-driven development CLI (idd core + spec engine + runner + TUI)",
    subcommand_required = true,
    arg_required_else_help = true
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    // ---- Core verbs: delegate verbatim to rusty_idd_core::cli::run.
    /// Initialize an IDD workspace (delegates to `idd init`).
    Init(CoreArgs),
    /// Scan a repository inventory (delegates to `idd scan`).
    Scan(CoreArgs),
    /// Generate a merge plan (delegates to `idd plan`).
    Plan(CoreArgs),
    /// Generate gated tasks (delegates to `idd task`).
    Task(CoreArgs),
    /// Run validation checks (delegates to `idd validate`).
    Validate(CoreArgs),
    /// Write the file manifest (delegates to `idd manifest`).
    Manifest(CoreArgs),
    /// GitHub agent orchestration (delegates to `idd github`).
    Github(CoreArgs),

    // ---- Grouped subcommands over the new libraries.
    /// Spec engine: validate / archive / show OpenSpec specs and deltas.
    #[command(subcommand)]
    Spec(commands::spec::SpecCommand),

    /// Headless task runner for an OpenSpec change (no interactive UI).
    Run(commands::run::RunArgs),

    /// Launch the interactive OpenSpec TUI.
    Tui,
}

/// Raw passthrough args for a core verb. Everything after the verb is captured
/// untouched (including `--flags`) and forwarded to `idd`'s own parser, so the
/// behaviour matches `idd <verb> ...` exactly.
#[derive(clap::Args)]
struct CoreArgs {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

/// Entry point: parse argv and dispatch. Returns the process exit code.
pub fn run() -> i32 {
    let cli = Cli::parse();
    dispatch(cli.command)
}

fn dispatch(command: Command) -> i32 {
    match command {
        Command::Init(a) => commands::core::delegate("init", &a.args),
        Command::Scan(a) => commands::core::delegate("scan", &a.args),
        Command::Plan(a) => commands::core::delegate("plan", &a.args),
        Command::Task(a) => commands::core::delegate("task", &a.args),
        Command::Validate(a) => commands::core::delegate("validate", &a.args),
        Command::Manifest(a) => commands::core::delegate("manifest", &a.args),
        Command::Github(a) => commands::core::delegate("github", &a.args),
        Command::Spec(cmd) => commands::spec::run(cmd),
        Command::Run(args) => commands::run::run(args),
        Command::Tui => commands::tui::run(),
    }
}
