//! `rusty-idd spec adr` — the FS edge over the ADR supersession graph
//! (`rusty_idd_spec::adr`). Reads the repo-level `adr/` directory and reports
//! the in-force decisions or the next sequence number.

use std::path::Path;

use clap::Subcommand;
use rusty_idd_spec::adr::{parse_adr, Adr, AdrSet, AdrStatus};

#[derive(Subcommand)]
pub enum AdrCommand {
    /// List ADRs. By default shows only the in-force set (accepted, not
    /// superseded); `--all` shows every ADR with its status.
    List {
        /// The ADR directory (defaults to `adr`).
        #[arg(default_value = "adr")]
        adr_dir: std::path::PathBuf,
        /// Show every ADR (including superseded/proposed) with its status.
        #[arg(long)]
        all: bool,
    },
    /// Print the next ADR sequence number (zero-padded NNNN).
    Next {
        /// The ADR directory (defaults to `adr`).
        #[arg(default_value = "adr")]
        adr_dir: std::path::PathBuf,
    },
}

/// Dispatch a `spec adr` subcommand.
pub fn run(cmd: AdrCommand) -> i32 {
    match cmd {
        AdrCommand::List { adr_dir, all } => run_list(&adr_dir, all),
        AdrCommand::Next { adr_dir } => run_next(&adr_dir),
    }
}

/// Read and parse every `NNNN-*.md` ADR file in `adr_dir`.
fn load_adrs(adr_dir: &Path) -> Result<AdrSet, String> {
    if !adr_dir.is_dir() {
        return Err(format!("adr directory not found: {}", adr_dir.display()));
    }
    let entries = std::fs::read_dir(adr_dir)
        .map_err(|e| format!("failed to read {}: {e}", adr_dir.display()))?;
    let mut adrs: Vec<Adr> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Only ADR files (start with a digit); skip README.md, templates, etc.
        if !name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        if let Some(a) = parse_adr(&name, &src) {
            adrs.push(a);
        }
    }
    Ok(AdrSet::new(adrs))
}

fn run_list(adr_dir: &Path, all: bool) -> i32 {
    let set = match load_adrs(adr_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rusty-idd: {e}");
            return 1;
        }
    };

    if all {
        let mut adrs: Vec<&Adr> = set.adrs.iter().collect();
        adrs.sort_by_key(|a| a.number);
        if adrs.is_empty() {
            println!("No ADRs in {}.", adr_dir.display());
            return 0;
        }
        for a in adrs {
            let status = status_label(&set, a);
            println!("ADR-{:04}  {:<11} {}", a.number, status, a.title);
        }
    } else {
        let in_force = set.in_force();
        if in_force.is_empty() {
            println!("No in-force ADRs in {}.", adr_dir.display());
            return 0;
        }
        println!("In-force ADRs ({}):", in_force.len());
        for a in in_force {
            println!("  ADR-{:04}  {}", a.number, a.title);
        }
    }
    0
}

/// A human status label for `--all` listing.
fn status_label(set: &AdrSet, a: &Adr) -> String {
    match &a.status {
        AdrStatus::Proposed => "proposed".to_string(),
        AdrStatus::Other(s) if s.is_empty() => "unknown".to_string(),
        AdrStatus::Other(s) => s.clone(),
        AdrStatus::Accepted => {
            // Find who supersedes it, if anyone.
            if let Some(by) = set
                .adrs
                .iter()
                .find(|other| other.supersedes.contains(&a.number))
            {
                format!("superseded(by {:04})", by.number)
            } else {
                "in-force".to_string()
            }
        }
    }
}

fn run_next(adr_dir: &Path) -> i32 {
    // A missing adr/ dir is fine for `next` — it just means start at 0001.
    let set = load_adrs(adr_dir).unwrap_or_default();
    println!("{:04}", set.next_number());
    0
}
