//! `rusty-idd spec scaffold` / `spec new` — the FS edge over the scaffold engine
//! (`rusty_idd_spec::scaffold`). Renders artifact stubs and (for `new`) creates
//! a change directory seeded with a proposal.

use std::path::{Path, PathBuf};

use rusty_idd_spec::scaffold::{render, ScaffoldContext};

/// `spec scaffold <artifact>` — render a stub to stdout (proposal/spec/design/
/// adr/tasks), with optional context for naming/ADR fields.
pub fn run_scaffold(
    artifact: &str,
    change: Option<String>,
    number: Option<String>,
    title: Option<String>,
    date: Option<String>,
) -> i32 {
    let mut ctx = ScaffoldContext::default();
    if let Some(c) = change {
        ctx.change = c;
    }
    if let Some(n) = number {
        ctx.number = n;
    }
    if let Some(t) = title {
        ctx.title = t;
    }
    if let Some(d) = date {
        ctx.date = d;
    }
    match render(artifact, &ctx) {
        Ok(s) => {
            print!("{s}");
            0
        }
        Err(e) => {
            eprintln!("rusty-idd: {e}");
            1
        }
    }
}

/// `spec new <change>` — create `<base>/openspec/changes/<change>/proposal.md`
/// (the entry artifact), seeded with the change name. Does not overwrite.
pub fn run_new(change: &str, base: &Path) -> i32 {
    let change_dir: PathBuf = base.join("openspec/changes").join(change);
    let proposal = change_dir.join("proposal.md");
    if proposal.exists() {
        eprintln!(
            "rusty-idd: refusing to overwrite existing {}",
            proposal.display()
        );
        return 1;
    }
    let body = match render("proposal", &ScaffoldContext::for_change(change)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rusty-idd: {e}");
            return 1;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&change_dir) {
        eprintln!("rusty-idd: failed to create {}: {e}", change_dir.display());
        return 1;
    }
    if let Err(e) = std::fs::write(&proposal, body) {
        eprintln!("rusty-idd: failed to write {}: {e}", proposal.display());
        return 1;
    }
    println!("Created change '{change}':");
    println!("  {}", proposal.display());
    println!(
        "Next: edit the proposal, then `rusty-idd spec next {}`.",
        change_dir.display()
    );
    0
}
