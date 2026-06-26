// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `hf index` (PRD §8/§9) and `hf plan` (PRD §9) — repo navigation maps + the task DAG.
//!
//! HFTASK-0083 (ADR-0019 D5 #4): peeled into the `handoff-index` crate after the card loader
//! moved to handoff-core. `hf` aliases it as `index` so `index::cmd_index` / `index::cmd_plan`
//! stay valid. Depends only on handoff-core + work-order + serde_json.
//!
//! HFTASK-0050 was marked Done before either verb existed (its acceptance only ran a generic
//! `cargo test`, which never exercised the feature — a false-Done caught by the code-research run).
//! This module makes the Done true: `hf index` generates `.handoff/maps/{repo,test,owner,
//! dependency}-map.json` + a nav README so a cold-start agent can understand the repo from
//! generated files, and `hf plan` builds/refreshes the task DAG (topological order + ready/blocked).
//!
//! Every map is derived from REAL data the kernel already holds — the Cargo workspace, the source
//! tree, an optional CODEOWNERS, and the witnessed task cards/ledger — never fabricated. The pure
//! `build_*` functions are unit-tested with synthetic inputs (no filesystem).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use serde_json::{Value, json};
use work_order::{Status, WorkOrder};

use handoff_core::{current_statuses, load_tasks, status_of};

const MAPS: &str = ".handoff/maps";

// --- workspace / source discovery -------------------------------------------

/// Parse the root `Cargo.toml` workspace `members = [...]` list (no toml dep — the members line
/// is a simple bracketed string list). Returns the member directory names in declared order.
fn workspace_members(cargo_toml: &str) -> Vec<String> {
    let Some(start) = cargo_toml.find("members") else {
        return vec![];
    };
    let tail = &cargo_toml[start..];
    let Some(lb) = tail.find('[') else {
        return vec![];
    };
    let Some(rb) = tail[lb..].find(']') else {
        return vec![];
    };
    tail[lb + 1..lb + rb]
        .split(',')
        .filter_map(|s| {
            let t = s.trim().trim_matches('"').trim();
            (!t.is_empty()).then(|| t.to_string())
        })
        .collect()
}

/// Recursively list `*.rs` files under `dir` (repo-relative paths). Best-effort: an unreadable
/// directory contributes nothing rather than aborting the whole index.
fn rust_files(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            if p.file_name().and_then(|s| s.to_str()) == Some("target") {
                continue;
            }
            rust_files(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p.to_string_lossy().replace("./", ""));
        }
    }
}

// --- pure map builders (unit-tested) ----------------------------------------

/// repo-map: the crates and their source files (PRD §8 navigation index input).
fn build_repo_map(members: &[(String, Vec<String>)]) -> Value {
    let crates: Vec<Value> = members
        .iter()
        .map(
            |(name, files)| json!({ "crate": name, "src_files": files, "file_count": files.len() }),
        )
        .collect();
    json!({
        "schema": "handoff.repo_map.v1",
        "source": "Cargo.toml workspace members",
        "crate_count": crates.len(),
        "crates": crates,
    })
}

/// test-map: source files that contain a test harness (`#[test]` / `#[tokio::test]`) or live under
/// a `tests/` integration dir — what maps acceptance criteria to executable evidence (PRD §8).
fn build_test_map(test_files: &[String]) -> Value {
    json!({
        "schema": "handoff.test_map.v1",
        "source": "files with #[test]/#[tokio::test] or under tests/",
        "count": test_files.len(),
        "test_files": test_files,
    })
}

/// owner-map: path-prefix → owner. Real source only: parsed CODEOWNERS lines (`pattern owner...`).
/// With no CODEOWNERS the map is empty + flagged — honest absence, never a fabricated owner.
fn build_owner_map(codeowners: Option<&str>) -> Value {
    let mut owners: BTreeMap<String, Vec<String>> = BTreeMap::new();
    if let Some(text) = codeowners {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            if let Some(pat) = it.next() {
                let who: Vec<String> = it.map(|s| s.to_string()).collect();
                if !who.is_empty() {
                    owners.insert(pat.to_string(), who);
                }
            }
        }
    }
    json!({
        "schema": "handoff.owner_map.v1",
        "source": if codeowners.is_some() { "CODEOWNERS" } else { "none (add .github/CODEOWNERS to assign owners)" },
        "owner_count": owners.len(),
        "owners": owners,
    })
}

/// The task DAG (PRD §9): topological order, the currently-ready set (deps Done), and per-task
/// unmet dependencies. Pure over the cards + replayed statuses. Kahn's algorithm; any cards left
/// after the queue drains form a cycle and are reported under `cyclic`.
pub struct TaskDag {
    pub order: Vec<String>,
    pub ready: Vec<String>,
    pub blocked: BTreeMap<String, Vec<String>>,
    pub cyclic: Vec<String>,
}

fn build_task_dag(tasks: &[WorkOrder], replay: &[(String, Status)]) -> TaskDag {
    let ids: BTreeSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let done = |id: &str| replay.iter().any(|(k, s)| k == id && *s == Status::Done);

    // unmet deps = a dependency that is a known task and not Done.
    let mut unmet: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for t in tasks {
        let u: Vec<String> = t
            .dependencies
            .iter()
            .filter(|d| ids.contains(d.as_str()) && !done(d))
            .cloned()
            .collect();
        unmet.insert(t.id.clone(), u);
    }

    // Kahn topo over the not-yet-Done tasks (Done tasks are already satisfied edges).
    let mut remaining: BTreeSet<String> = tasks
        .iter()
        .filter(|t| !done(&t.id))
        .map(|t| t.id.clone())
        .collect();
    let mut resolved: HashSet<String> = tasks
        .iter()
        .filter(|t| done(&t.id))
        .map(|t| t.id.clone())
        .collect();
    let mut order = vec![];
    loop {
        // tasks whose every known dependency is resolved (Done or already ordered).
        let mut layer: Vec<String> = remaining
            .iter()
            .filter(|id| {
                tasks
                    .iter()
                    .find(|t| &t.id == *id)
                    .map(|t| {
                        t.dependencies
                            .iter()
                            .all(|d| !ids.contains(d.as_str()) || resolved.contains(d))
                    })
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        if layer.is_empty() {
            break;
        }
        layer.sort();
        for id in &layer {
            remaining.remove(id);
            resolved.insert(id.clone());
            order.push(id.clone());
        }
    }
    let cyclic: Vec<String> = remaining.into_iter().collect();

    // ready = ordered tasks with zero unmet deps that aren't themselves Done.
    let ready: Vec<String> = order
        .iter()
        .filter(|id| unmet.get(*id).map(|u| u.is_empty()).unwrap_or(true))
        .cloned()
        .collect();

    let blocked: BTreeMap<String, Vec<String>> =
        unmet.into_iter().filter(|(_, u)| !u.is_empty()).collect();

    TaskDag {
        order,
        ready,
        blocked,
        cyclic,
    }
}

fn dag_to_json(dag: &TaskDag) -> Value {
    json!({
        "schema": "handoff.task_dag.v1",
        "topological_order": dag.order,
        "ready": dag.ready,
        "blocked": dag.blocked,
        "cyclic": dag.cyclic,
    })
}

/// dependency-map: per-task dependency/blocked-by edges + replayed status (PRD §8 dependency map).
fn build_dependency_map(tasks: &[WorkOrder], replay: &[(String, Status)]) -> Value {
    let mut nodes: BTreeMap<String, Value> = BTreeMap::new();
    let mut edges: Vec<Value> = vec![];
    for t in tasks {
        let st = status_of(&t.id, replay, t);
        nodes.insert(
            t.id.clone(),
            json!({
                "status": format!("{st:?}"),
                "dependencies": t.dependencies,
                "blocked_by": t.blocked_by,
            }),
        );
        for d in &t.dependencies {
            edges.push(json!({ "from": t.id, "to": d, "kind": "depends_on" }));
        }
    }
    json!({
        "schema": "handoff.dependency_map.v1",
        "node_count": nodes.len(),
        "edge_count": edges.len(),
        "nodes": nodes,
        "edges": edges,
    })
}

// --- IO + verbs -------------------------------------------------------------

fn write_map(name: &str, value: &Value) -> std::io::Result<()> {
    fs::create_dir_all(MAPS)?;
    let path = format!("{MAPS}/{name}");
    fs::write(&path, format!("{}\n", handoff_core::pretty_json(value)))
}

/// `hf index` — generate the navigation maps under `.handoff/maps/`. Fail-closed: a write error
/// exits non-zero (the maps are continuity-gating navigation state, not best-effort scratch).
pub fn cmd_index() {
    let cargo_toml = fs::read_to_string("Cargo.toml").unwrap_or_default();
    let members = workspace_members(&cargo_toml);
    let member_files: Vec<(String, Vec<String>)> = members
        .iter()
        .map(|m| {
            let mut files = vec![];
            rust_files(&Path::new(m).join("src"), &mut files);
            files.sort();
            (m.clone(), files)
        })
        .collect();

    // test files = anything with a test attribute or under a tests/ dir.
    let mut all_rs = vec![];
    for (m, _) in &member_files {
        rust_files(Path::new(m), &mut all_rs);
    }
    let mut test_files: Vec<String> = all_rs
        .into_iter()
        .filter(|f| {
            f.contains("/tests/")
                || fs::read_to_string(f)
                    .map(|s| s.contains("#[test]") || s.contains("#[tokio::test]"))
                    .unwrap_or(false)
        })
        .collect();
    test_files.sort();
    test_files.dedup();

    let codeowners = fs::read_to_string(".github/CODEOWNERS").ok();
    let tasks = load_tasks();
    let replay = current_statuses();

    let maps: [(&str, Value); 4] = [
        ("repo-map.json", build_repo_map(&member_files)),
        ("test-map.json", build_test_map(&test_files)),
        ("owner-map.json", build_owner_map(codeowners.as_deref())),
        ("dependency-map.json", build_dependency_map(&tasks, &replay)),
    ];
    for (name, value) in &maps {
        if let Err(e) = write_map(name, value) {
            eprintln!("hf index: cannot write {MAPS}/{name}: {e}");
            std::process::exit(1);
        }
    }

    // nav README so the maps are self-describing for a cold-start agent.
    let readme = format!(
        "# `.handoff/maps/` — generated navigation index\n\n\
         Generated by `hf index` (PRD §8/§9). Do not hand-edit; re-run `hf index`.\n\n\
         - `repo-map.json` — {} crate(s) and their source files.\n\
         - `test-map.json` — {} file(s) carrying tests.\n\
         - `owner-map.json` — path → owner (from CODEOWNERS).\n\
         - `dependency-map.json` — task graph (nodes, edges, status).\n\n\
         Run `hf plan` for the topological task DAG (`task-dag.json`).\n",
        member_files.len(),
        test_files.len()
    );
    if let Err(e) = fs::write(format!("{MAPS}/README.md"), readme) {
        eprintln!("hf index: cannot write {MAPS}/README.md: {e}");
        std::process::exit(1);
    }

    println!(
        "hf index: wrote {}/{{repo,test,owner,dependency}}-map.json + README.md ({} crates, {} test files)",
        MAPS,
        member_files.len(),
        test_files.len()
    );
}

/// `hf plan` — build/refresh the task DAG (`.handoff/maps/task-dag.json`) and print the plan.
/// Fail-closed on a write error.
pub fn cmd_plan(json_out: bool) {
    let tasks = load_tasks();
    let replay = current_statuses();
    let dag = build_task_dag(&tasks, &replay);
    let value = dag_to_json(&dag);

    if let Err(e) = write_map("task-dag.json", &value) {
        eprintln!("hf plan: cannot write {MAPS}/task-dag.json: {e}");
        std::process::exit(1);
    }

    if json_out {
        println!("{}", handoff_core::pretty_json(&value));
        return;
    }
    println!(
        "hf plan: {} task(s) ordered → {}/task-dag.json",
        dag.order.len(),
        MAPS
    );
    if !dag.ready.is_empty() {
        println!("ready ({}): {}", dag.ready.len(), dag.ready.join(", "));
    }
    if !dag.blocked.is_empty() {
        println!("blocked:");
        for (id, deps) in &dag.blocked {
            println!("  {id} ← {}", deps.join(", "));
        }
    }
    if !dag.cyclic.is_empty() {
        println!("⚠ cyclic (unorderable): {}", dag.cyclic.join(", "));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use work_order::WorkOrder;

    fn wo(id: &str, deps: &[&str]) -> WorkOrder {
        WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: id.into(),
            status: Status::Backlog,
            priority: work_order::Priority::P2,
            objective: "obj".into(),
            path_scope: vec!["**".into()],
            acceptance_criteria: vec!["ok".into()],
            test_commands: vec![],
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: String::new(),
            role: None,
            intent_lock: WorkOrder::compute_intent_lock("obj", &["**".into()], &["ok".into()]),
        }
    }

    #[test]
    fn workspace_members_parses_bracketed_list() {
        let toml =
            "[workspace]\nresolver = \"2\"\nmembers = [\"work-order\", \"ledger\", \"hf\"]\n";
        assert_eq!(workspace_members(toml), vec!["work-order", "ledger", "hf"]);
        assert!(workspace_members("[workspace]\n").is_empty());
    }

    #[test]
    fn repo_and_test_maps_have_expected_shape() {
        let members = vec![("hf".to_string(), vec!["hf/src/main.rs".to_string()])];
        let rm = build_repo_map(&members);
        assert_eq!(rm["schema"], "handoff.repo_map.v1");
        assert_eq!(rm["crate_count"], 1);
        assert_eq!(rm["crates"][0]["file_count"], 1);

        let tm = build_test_map(&["hf/tests/cli.rs".to_string()]);
        assert_eq!(tm["schema"], "handoff.test_map.v1");
        assert_eq!(tm["count"], 1);
    }

    #[test]
    fn owner_map_parses_codeowners_and_flags_absence() {
        let co = build_owner_map(Some("# comment\n/hf/ @alice @bob\n*.md @docs\n"));
        assert_eq!(co["owner_count"], 2);
        assert_eq!(co["owners"]["/hf/"][0], "@alice");
        let none = build_owner_map(None);
        assert_eq!(none["owner_count"], 0);
        assert!(none["source"].as_str().unwrap().contains("none"));
    }

    #[test]
    fn task_dag_orders_by_dependency_and_finds_ready() {
        // C depends on B depends on A; nothing Done → A is the only ready, topo order A,B,C.
        let tasks = vec![
            wo("TASK-C", &["TASK-B"]),
            wo("TASK-B", &["TASK-A"]),
            wo("TASK-A", &[]),
        ];
        let replay = vec![]; // nothing Done
        let dag = build_task_dag(&tasks, &replay);
        assert_eq!(dag.order, vec!["TASK-A", "TASK-B", "TASK-C"]);
        assert_eq!(dag.ready, vec!["TASK-A"]);
        assert_eq!(dag.blocked["TASK-B"], vec!["TASK-A"]);
        assert!(dag.cyclic.is_empty());
    }

    #[test]
    fn task_dag_respects_done_dependencies() {
        // A Done → B becomes ready.
        let tasks = vec![wo("TASK-B", &["TASK-A"]), wo("TASK-A", &[])];
        let replay = vec![("TASK-A".to_string(), Status::Done)];
        let dag = build_task_dag(&tasks, &replay);
        assert_eq!(dag.ready, vec!["TASK-B"]); // A is Done so it drops out; B is now ready
        assert!(dag.blocked.is_empty());
    }

    #[test]
    fn task_dag_reports_cycles() {
        let tasks = vec![wo("TASK-X", &["TASK-Y"]), wo("TASK-Y", &["TASK-X"])];
        let dag = build_task_dag(&tasks, &[]);
        assert_eq!(dag.cyclic.len(), 2);
        assert!(dag.order.is_empty());
    }

    #[test]
    fn dependency_map_emits_nodes_and_edges() {
        let tasks = vec![wo("TASK-B", &["TASK-A"]), wo("TASK-A", &[])];
        let dm = build_dependency_map(&tasks, &[]);
        assert_eq!(dm["node_count"], 2);
        assert_eq!(dm["edge_count"], 1);
        assert_eq!(dm["edges"][0]["to"], "TASK-A");
    }
}
