//! `rusty-idd spec archive` — the filesystem-owning half of the archive flow
//! that the spec lib deferred to the CLI (design §5).
//!
//! Flow:
//! 1. Discover delta `spec.md` files under `<change>/specs/<cap>/spec.md`.
//! 2. Resolve each capability's base spec under `<openspec>/specs/<cap>/spec.md`.
//! 3. Drive the transactional [`rusty_idd_spec::archive::archive_specs`] merge:
//!    if ANY merge errors, write NOTHING and abort.
//! 4. (default) Validate the merged result specs; abort on any ERROR.
//!    `--no-validate` skips this. `-y` bypasses the interactive confirmation.
//! 5. On success: write the merged base specs, then move the change dir to
//!    `<changes>/archive/<change>/`.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use rusty_idd_runner::data::discover_specs;
use rusty_idd_spec::archive::{archive_specs, SpecMerge};
use rusty_idd_spec::validate::{validate_spec, IssueLevel};

/// Run `spec archive`. Returns a process exit code.
///
/// - `no_validate`: skip the default pre-archive validation of the merged specs.
/// - `yes`: assume "yes" and skip the interactive confirmation prompt.
pub fn run(change_dir: &Path, skip_specs: bool, no_validate: bool, yes: bool) -> i32 {
    if !change_dir.is_dir() {
        eprintln!(
            "rusty-idd: change directory not found: {}",
            change_dir.display()
        );
        return 1;
    }

    if skip_specs {
        if !confirm_archive(change_dir, 0, yes) {
            println!("Aborted by user. No files were changed.");
            return 1;
        }
        return move_change_dir(change_dir);
    }

    // 1. Discover delta specs in the change.
    let specs = discover_specs(change_dir);
    if specs.is_empty() {
        // No delta specs to merge — just move the change dir.
        println!("No delta specs found under {}.", change_dir.display());
        return move_change_dir(change_dir);
    }

    // 2. Resolve base specs and read sources. We must read EVERYTHING up front
    //    so the transactional driver sees all pairs before any write.
    let specs_root = match base_specs_root(change_dir) {
        Some(p) => p,
        None => {
            eprintln!(
                "rusty-idd: could not locate the base specs root for {}",
                change_dir.display()
            );
            return 1;
        }
    };

    struct Loaded {
        capability: String,
        base_path: PathBuf,
        base_src: String,
        delta_src: String,
    }

    let mut loaded: Vec<Loaded> = Vec::with_capacity(specs.len());
    for item in &specs {
        let base_path = specs_root.join(&item.name).join("spec.md");
        let base_src = match std::fs::read_to_string(&base_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "rusty-idd: failed to read base spec {}: {e}",
                    base_path.display()
                );
                return 1;
            }
        };
        let delta_src = match std::fs::read_to_string(&item.path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!(
                    "rusty-idd: failed to read delta spec {}: {e}",
                    item.path.display()
                );
                return 1;
            }
        };
        loaded.push(Loaded {
            capability: item.name.clone(),
            base_path,
            base_src,
            delta_src,
        });
    }

    // 3. Transactional merge — abort-all-on-any-failure, write nothing on error.
    let merges: Vec<SpecMerge<'_>> = loaded
        .iter()
        .map(|l| SpecMerge {
            capability: &l.capability,
            base_src: &l.base_src,
            delta_src: &l.delta_src,
        })
        .collect();

    let merged = match archive_specs(&merges) {
        Ok(m) => m,
        Err(err) => {
            eprintln!("rusty-idd: {err}");
            eprintln!("Aborted. No files were changed.");
            return 1;
        }
    };

    // 3b. (default) Validate the merged RESULT specs before writing anything.
    //     A merge can yield a structurally-invalid spec (e.g. a MODIFIED block
    //     that drops all scenarios); the oracle validates the change before
    //     archiving. `--no-validate` skips this. Only ERRORs abort (WARNINGs are
    //     surfaced but non-fatal, matching `spec validate` non-strict semantics).
    if !no_validate {
        let mut had_error = false;
        for result in &merged {
            let report = validate_spec(&result.capability, &result.markdown);
            for item in &report.items {
                for issue in &item.issues {
                    if issue.level == IssueLevel::Error {
                        had_error = true;
                        eprintln!(
                            "rusty-idd: {} validation ERROR {}: {}",
                            result.capability, issue.path, issue.message
                        );
                    }
                }
            }
        }
        if had_error {
            eprintln!(
                "rusty-idd: merged specs failed validation. Re-run with --no-validate to skip."
            );
            eprintln!("Aborted. No files were changed.");
            return 1;
        }
    }

    // 3c. Confirm before any destructive write (interactive only; -y bypasses).
    if !confirm_archive(change_dir, merged.len(), yes) {
        println!("Aborted by user. No files were changed.");
        return 1;
    }

    // 4a. Write merged base specs (all merges already succeeded in memory).
    for result in &merged {
        // Find the matching base path by capability.
        let base_path = loaded
            .iter()
            .find(|l| l.capability == result.capability)
            .map(|l| l.base_path.clone());
        let Some(base_path) = base_path else {
            // Unreachable: every merged result comes from a loaded entry.
            eprintln!(
                "rusty-idd: internal error: no base path for {}",
                result.capability
            );
            return 1;
        };
        if let Err(e) = std::fs::write(&base_path, &result.markdown) {
            eprintln!(
                "rusty-idd: failed to write merged spec {}: {e}",
                base_path.display()
            );
            return 1;
        }
        let c = result.counts;
        println!(
            "  {} (+{} ~{} -{} →{})",
            result.capability, c.added, c.modified, c.removed, c.renamed
        );
    }

    // 4b. Move the change dir into archive/.
    move_change_dir(change_dir)
}

/// The base specs root for a change at `<openspec>/changes/<change>/` is
/// `<openspec>/specs/`. We compute it as `change_dir/../../specs` and verify it
/// exists.
fn base_specs_root(change_dir: &Path) -> Option<PathBuf> {
    let changes_dir = change_dir.parent()?; // .../changes
    let openspec_root = changes_dir.parent()?; // .../openspec (or repo root)
    let candidate = openspec_root.join("specs");
    if candidate.is_dir() {
        Some(candidate)
    } else {
        None
    }
}

/// Move `<changes>/<change>/` to `<changes>/archive/<change>/`.
fn move_change_dir(change_dir: &Path) -> i32 {
    let Some(name) = change_dir.file_name() else {
        eprintln!("rusty-idd: change directory has no name component");
        return 1;
    };
    let Some(changes_dir) = change_dir.parent() else {
        eprintln!("rusty-idd: change directory has no parent");
        return 1;
    };
    let archive_dir = changes_dir.join("archive");
    if let Err(e) = std::fs::create_dir_all(&archive_dir) {
        eprintln!(
            "rusty-idd: failed to create archive dir {}: {e}",
            archive_dir.display()
        );
        return 1;
    }
    let dest = archive_dir.join(name);
    if dest.exists() {
        eprintln!(
            "rusty-idd: archive destination already exists: {}",
            dest.display()
        );
        return 1;
    }
    if let Err(e) = std::fs::rename(change_dir, &dest) {
        eprintln!(
            "rusty-idd: failed to move {} -> {}: {e}",
            change_dir.display(),
            dest.display()
        );
        return 1;
    }
    println!("Archived change to {}.", dest.display());
    0
}

/// Whether a confirmation prompt is needed: only when NOT `-y` AND stdin is an
/// interactive terminal. Non-interactive callers (CI, scripts, tests) proceed
/// without prompting, preserving the prior non-interactive behavior. Pure so it
/// is unit-testable.
fn needs_prompt(yes: bool, is_terminal: bool) -> bool {
    !yes && is_terminal
}

/// Ask the user to confirm a destructive archive. Returns `true` to proceed.
/// Bypassed (returns `true`) when `-y` is set or stdin is not a TTY.
fn confirm_archive(change_dir: &Path, spec_count: usize, yes: bool) -> bool {
    if !needs_prompt(yes, io::stdin().is_terminal()) {
        return true;
    }
    let name = change_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| change_dir.display().to_string());
    eprint!(
        "Archive change '{name}'? This rewrites {spec_count} base spec(s) and moves the change dir. [y/N] "
    );
    let _ = io::stderr().flush();
    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return false;
    }
    matches!(line.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::needs_prompt;

    #[test]
    fn yes_flag_never_prompts() {
        assert!(!needs_prompt(true, true));
        assert!(!needs_prompt(true, false));
    }

    #[test]
    fn non_tty_never_prompts() {
        assert!(!needs_prompt(false, false));
    }

    #[test]
    fn interactive_without_yes_prompts() {
        assert!(needs_prompt(false, true));
    }
}
