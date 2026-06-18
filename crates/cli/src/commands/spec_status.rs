//! `rusty-idd spec status` / `spec next` — the FS edge over the spec engine's
//! artifact DAG (`rusty_idd_spec::schema`, design §4).
//!
//! Determines which artifacts a change has produced (by globbing the change dir)
//! and feeds that `done` set into the schema graph to report status, the next
//! ready artifact, and archivability.

use std::collections::BTreeSet;
use std::path::Path;

use rusty_idd_spec::schema::{load_schema, Schema};

/// The canonical intent-driven schema, embedded as the default when a project
/// has no `openspec/schemas/intent-driven/schema.yaml` of its own.
const DEFAULT_SCHEMA: &str =
    include_str!("../../../../intent-driven-template/openspec/schemas/intent-driven/schema.yaml");

/// Load the schema for a change: prefer the project's
/// `<openspec>/schemas/intent-driven/schema.yaml` (change_dir/../../schemas/...),
/// else the embedded default.
fn load_for(change_dir: &Path) -> Result<Schema, String> {
    let project = change_dir
        .parent() // .../changes
        .and_then(|p| p.parent()) // .../openspec (or repo root)
        .map(|root| root.join("schemas/intent-driven/schema.yaml"));
    if let Some(path) = project {
        if path.is_file() {
            let src = std::fs::read_to_string(&path)
                .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
            return load_schema(&src).map_err(|e| e.to_string());
        }
    }
    load_schema(DEFAULT_SCHEMA).map_err(|e| e.to_string())
}

/// Compute the set of artifact ids that are `done` for this change: an artifact
/// is done when its `generates` glob matches at least one file (relative to the
/// change dir).
fn done_set(change_dir: &Path, schema: &Schema) -> BTreeSet<String> {
    schema
        .artifacts
        .iter()
        .filter(|a| generates_matches(change_dir, &a.generates))
        .map(|a| a.id.clone())
        .collect()
}

/// Does a `generates` glob match any file, resolved relative to `change_dir`?
/// Supports the three shapes the schema uses: an exact path (`proposal.md`), a
/// recursive glob (`specs/**/*.md`), and a single-level glob (`../../../adr/*.md`).
fn generates_matches(change_dir: &Path, generates: &str) -> bool {
    if let Some(idx) = generates.find("**") {
        let prefix = generates[..idx].trim_end_matches('/');
        let ext = extension_of(generates);
        return any_file_with_ext(&change_dir.join(prefix), ext.as_deref(), true);
    }
    if generates.contains('*') {
        let p = change_dir.join(generates);
        let dir = p.parent().unwrap_or(change_dir).to_path_buf();
        let ext = extension_of(generates);
        return any_file_with_ext(&dir, ext.as_deref(), false);
    }
    change_dir.join(generates).exists()
}

/// The lowercase extension of a glob's final component (`*.md` → `md`).
fn extension_of(glob: &str) -> Option<String> {
    glob.rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
}

/// Is there a file (optionally recursing) under `dir` whose extension matches
/// `ext` (or any file if `ext` is `None`)?
fn any_file_with_ext(dir: &Path, ext: Option<&str>, recurse: bool) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if recurse && any_file_with_ext(&path, ext, true) {
                return true;
            }
            continue;
        }
        match ext {
            None => return true,
            Some(want) => {
                let got = path
                    .extension()
                    .map(|e| e.to_string_lossy().to_ascii_lowercase());
                if got.as_deref() == Some(want) {
                    return true;
                }
            }
        }
    }
    false
}

/// `spec status <change_dir>` — print each artifact's done/ready state plus the
/// archivability verdict and the next ready artifact.
pub fn run_status(change_dir: &Path) -> i32 {
    if !change_dir.is_dir() {
        eprintln!(
            "rusty-idd: change directory not found: {}",
            change_dir.display()
        );
        return 1;
    }
    let schema = match load_for(change_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rusty-idd: {e}");
            return 1;
        }
    };
    let done = done_set(change_dir, &schema);

    let name = change_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| change_dir.display().to_string());
    println!(
        "Change: {name}  (schema: {} v{})",
        schema.name, schema.version
    );

    let order = match schema.topo_order() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("rusty-idd: {e}");
            return 1;
        }
    };
    let next = schema.next_ready(&done).map(|a| a.id.clone());
    for a in &order {
        let is_done = done.contains(&a.id);
        let mark = if is_done { "x" } else { " " };
        let mut note = String::new();
        if Some(&a.id) == next.as_ref() {
            note = "  <- next".to_string();
        } else if !is_done {
            let blockers: Vec<&str> = a
                .requires
                .iter()
                .filter(|r| !done.contains(*r))
                .map(|r| r.as_str())
                .collect();
            if !blockers.is_empty() {
                note = format!("  (blocked by: {})", blockers.join(", "));
            }
        }
        println!("  [{mark}] {:<9} {}{}", a.id, a.generates, note);
    }

    let done_count = schema
        .artifacts
        .iter()
        .filter(|a| done.contains(&a.id))
        .count();
    let total = schema.artifacts.len();
    if schema.is_archivable(&done) {
        println!("Archivable: yes ({done_count}/{total} artifacts done)");
    } else {
        println!("Archivable: no ({done_count}/{total} artifacts done)");
    }
    match &next {
        Some(id) => println!("Next: {id}"),
        None if schema.is_archivable(&done) => println!("Next: (none — ready to archive)"),
        None => println!("Next: (none ready — blocked)"),
    }
    0
}

/// `spec next <change_dir>` — print just the next ready artifact id (scriptable),
/// or a note when the change is complete.
pub fn run_next(change_dir: &Path) -> i32 {
    if !change_dir.is_dir() {
        eprintln!(
            "rusty-idd: change directory not found: {}",
            change_dir.display()
        );
        return 1;
    }
    let schema = match load_for(change_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("rusty-idd: {e}");
            return 1;
        }
    };
    let done = done_set(change_dir, &schema);
    match schema.next_ready(&done) {
        Some(a) => {
            println!("{}", a.id);
            0
        }
        None if schema.is_archivable(&done) => {
            println!("(all artifacts complete — ready to archive)");
            0
        }
        None => {
            println!("(no artifact ready — blocked)");
            0
        }
    }
}
