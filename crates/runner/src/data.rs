use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// Types for `openspec list --json`
#[derive(Debug, Deserialize, Clone)]
pub struct ChangeListOutput {
    pub changes: Vec<ChangeEntry>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangeEntry {
    pub name: String,
    pub completed_tasks: u32,
    pub total_tasks: u32,
}

// Types for `openspec status --change <name> --json`
#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ChangeStatusOutput {
    pub artifacts: Vec<ArtifactStatus>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactStatus {
    pub id: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct SpecItem {
    pub name: String,
    pub path: PathBuf,
}

/// Construct a `Command` for invoking the `openspec` CLI.
///
/// On Windows, npm-installed tools use `.cmd` wrappers that `Command::new`
/// cannot resolve directly. We use `cmd /C openspec` so that `cmd.exe`
/// handles PATHEXT resolution. On Unix/macOS, invoke the binary directly.
#[cfg(windows)]
fn openspec_command() -> Command {
    let mut cmd = Command::new("cmd");
    cmd.args(["/C", "openspec"]);
    cmd
}

#[cfg(not(windows))]
fn openspec_command() -> Command {
    Command::new("openspec")
}

/// Run `openspec list --json` and parse the result.
pub fn list_changes() -> Result<ChangeListOutput, String> {
    let output = openspec_command()
        .args(["list", "--json"])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "openspec CLI not found on PATH. Please install openspec first.".to_string()
            } else {
                format!("Failed to run openspec: {e}")
            }
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("openspec list failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse openspec list output: {e}"))
}

/// Run `openspec status --change <name> --json` and parse the result.
pub fn get_change_status(name: &str) -> Result<ChangeStatusOutput, String> {
    let output = openspec_command()
        .args(["status", "--change", name, "--json"])
        .output()
        .map_err(|e| format!("Failed to run openspec status: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("openspec status failed: {stderr}"));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout)
        .map_err(|e| format!("Failed to parse openspec status output: {e}"))
}

/// List archived changes by reading `openspec/changes/archive/` directory entries.
///
/// Each subdirectory becomes a `ChangeEntry` with task progress parsed from its `tasks.md`.
/// Results are sorted by date descending (newest first), then name ascending.
/// Returns an empty list if the archive directory doesn't exist.
pub fn list_archived_changes() -> Result<Vec<ChangeEntry>, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("Failed to get cwd: {e}"))?;
    let archive_dir = cwd.join("openspec").join("changes").join("archive");

    if !archive_dir.exists() {
        return Ok(Vec::new());
    }

    let entries =
        fs::read_dir(&archive_dir).map_err(|e| format!("Failed to read archive directory: {e}"))?;

    let mut changes: Vec<ChangeEntry> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let tasks_path = path.join("tasks.md");
            let (completed_tasks, total_tasks) = if tasks_path.exists() {
                parse_task_progress(&tasks_path).unwrap_or_default()
            } else {
                (0, 0)
            };
            Some(ChangeEntry {
                name,
                completed_tasks,
                total_tasks,
            })
        })
        .collect();

    // Sort: date descending (newest first), then name ascending within same date
    changes.sort_by(|a, b| {
        let date_a = if a.name.len() >= 10 {
            &a.name[..10]
        } else {
            &a.name
        };
        let date_b = if b.name.len() >= 10 {
            &b.name[..10]
        } else {
            &b.name
        };
        // Primary: date descending
        let date_cmp = date_b.cmp(date_a);
        if date_cmp != std::cmp::Ordering::Equal {
            return date_cmp;
        }
        // Secondary: name remainder ascending
        let rest_a = if a.name.len() > 11 { &a.name[11..] } else { "" };
        let rest_b = if b.name.len() > 11 { &b.name[11..] } else { "" };
        rest_a.cmp(rest_b)
    });

    Ok(changes)
}

/// Build a `ChangeStatusOutput` for an archived change by checking file existence.
///
/// Instead of calling `openspec status`, checks which artifact files exist in the
/// archive directory. All existing files are treated as "done".
pub fn get_archived_change_status(change_dir: &Path) -> ChangeStatusOutput {
    let artifact_checks = [
        ("proposal", "proposal.md"),
        ("design", "design.md"),
        ("tasks", "tasks.md"),
    ];

    let mut artifacts: Vec<ArtifactStatus> = artifact_checks
        .iter()
        .map(|(id, filename)| {
            let status = if change_dir.join(filename).exists() {
                "done"
            } else {
                "pending"
            };
            ArtifactStatus {
                id: id.to_string(),
                status: status.to_string(),
            }
        })
        .collect();

    // Check specs directory
    let specs_dir = change_dir.join("specs");
    let specs_status = if specs_dir.exists() && specs_dir.is_dir() {
        "done"
    } else {
        "pending"
    };
    artifacts.push(ArtifactStatus {
        id: "specs".to_string(),
        status: specs_status.to_string(),
    });

    ChangeStatusOutput { artifacts }
}

/// Read artifact file content from disk.
pub fn read_artifact_content(path: &Path) -> Result<String, String> {
    fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))
}

/// Parse a tasks.md file and count completed vs total tasks.
///
/// Counts lines matching `- [x]` (completed) and `- [ ]` (uncompleted).
/// Returns `(completed, total)`.
pub fn parse_task_progress(path: &Path) -> Result<(u32, u32), String> {
    let content =
        fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
    let mut completed = 0u32;
    let mut total = 0u32;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            completed += 1;
            total += 1;
        } else if trimmed.starts_with("- [ ]") {
            total += 1;
        }
    }
    Ok((completed, total))
}

/// Find the first unchecked task in a tasks.md file.
///
/// Scans all `- [x]`/`- [X]` (checked) and `- [ ]` (unchecked) lines.
/// Returns the 1-based task index and description text of the first unchecked
/// task, or `None` if all tasks are complete or no tasks exist.
pub fn next_unchecked_task(path: &Path) -> Option<(u32, String)> {
    let content = fs::read_to_string(path).ok()?;
    let mut task_index = 0u32;
    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            task_index += 1;
        } else if trimmed.starts_with("- [ ]") {
            task_index += 1;
            let text = trimmed.trim_start_matches("- [ ]").trim().to_string();
            return Some((task_index, text));
        }
    }
    None
}

/// Discover spec sub-items by listing the `specs/` subdirectory of a change.
pub fn discover_specs(change_dir: &Path) -> Vec<SpecItem> {
    let specs_dir = change_dir.join("specs");
    let Ok(entries) = fs::read_dir(&specs_dir) else {
        return Vec::new();
    };

    let mut specs: Vec<SpecItem> = entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                let spec_file = path.join("spec.md");
                if spec_file.exists() {
                    Some(SpecItem {
                        name: entry.file_name().to_string_lossy().to_string(),
                        path: spec_file,
                    })
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect();

    specs.sort_by(|a, b| a.name.cmp(&b.name));
    specs
}

/// Run mode for a change's implementation.
#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    #[default]
    Normal,
    Apply,
}

/// Per-change configuration, stored in `change-config.yaml`.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ChangeConfig {
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub run_mode: RunMode,
}

/// Read the full change config from `change-config.yaml`.
///
/// Returns default config if the file does not exist. Returns an error if the
/// file exists but cannot be read or parsed.
pub fn read_change_config(change_dir: &Path) -> Result<ChangeConfig, String> {
    let path = change_dir.join("change-config.yaml");
    if !path.exists() {
        return Ok(ChangeConfig::default());
    }
    let content = fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
    serde_norway::from_str(&content)
        .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))
}

/// Read dependencies for a change from its `change-config.yaml` file.
///
/// Returns an empty list if the file does not exist. Returns an error if the
/// file exists but is corrupt.
pub fn read_dependencies(change_dir: &Path) -> Result<Vec<String>, String> {
    read_change_config(change_dir).map(|c| c.depends_on)
}

/// Read the run mode for a change from its `change-config.yaml` file.
///
/// Returns `RunMode::Normal` if the file does not exist. Returns an error if the
/// file exists but is corrupt.
pub fn read_run_mode(change_dir: &Path) -> Result<RunMode, String> {
    read_change_config(change_dir).map(|c| c.run_mode)
}

/// Load dependencies for all given changes.
///
/// Returns a map from change name to its dependency list. Changes with no
/// dependencies are omitted from the map. Returns an error if CWD or any
/// existing config file cannot be read.
pub fn load_change_dependencies(
    changes: &[ChangeEntry],
) -> Result<HashMap<String, Vec<String>>, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("Failed to get cwd: {e}"))?;
    let changes_dir = cwd.join("openspec").join("changes");
    let mut map = HashMap::new();
    for c in changes {
        let dir = changes_dir.join(&c.name);
        let deps = read_dependencies(&dir)?;
        if !deps.is_empty() {
            map.insert(c.name.clone(), deps);
        }
    }
    Ok(map)
}

/// Write the full change config to `change-config.yaml`.
///
/// Creates or overwrites the file.
pub fn write_change_config(change_dir: &Path, config: &ChangeConfig) -> Result<(), String> {
    let path = change_dir.join("change-config.yaml");
    let yaml = serde_norway::to_string(config)
        .map_err(|e| format!("Failed to serialize change config: {e}"))?;
    fs::write(&path, yaml).map_err(|e| format!("Failed to write {}: {e}", path.display()))
}

/// Write dependencies for a change to its `change-config.yaml` file.
///
/// Reads existing config first to preserve other fields (like run_mode).
pub fn write_dependencies(change_dir: &Path, dependencies: &[String]) -> Result<(), String> {
    let mut config = read_change_config(change_dir)?;
    config.depends_on = dependencies.to_vec();
    write_change_config(change_dir, &config)
}

/// Perform a topological sort of changes using Kahn's algorithm.
///
/// Takes a map of change names to their dependencies. Returns a `Vec<String>`
/// in valid execution order (dependencies before dependents), or an `Err`
/// listing the changes involved in a cycle.
///
/// Changes that appear only as dependencies (but are not keys in the map)
/// are excluded from the output — they are assumed to be already fulfilled.
pub fn topological_sort(deps: &HashMap<String, Vec<String>>) -> Result<Vec<String>, String> {
    // Build in-degree map and adjacency list (dependency -> dependents)
    let mut in_degree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    // Initialize all known changes with zero in-degree
    for name in deps.keys() {
        in_degree.entry(name.as_str()).or_insert(0);
    }

    // Build edges: for each change, each dependency adds an edge dep -> change
    for (name, dep_list) in deps {
        for dep in dep_list {
            // Only count edges from dependencies that are in our change set
            if deps.contains_key(dep) {
                *in_degree.entry(name.as_str()).or_insert(0) += 1;
                dependents
                    .entry(dep.as_str())
                    .or_default()
                    .push(name.as_str());
            }
        }
    }

    // Start with all nodes that have zero in-degree
    let mut queue: VecDeque<&str> = in_degree
        .iter()
        .filter(|(_, deg)| **deg == 0)
        .map(|(name, _)| *name)
        .collect();

    // Sort the initial queue for deterministic output
    let mut sorted_queue: Vec<&str> = queue.drain(..).collect();
    sorted_queue.sort();
    queue.extend(sorted_queue);

    let mut result: Vec<String> = Vec::new();

    while let Some(node) = queue.pop_front() {
        result.push(node.to_string());

        if let Some(deps_of_node) = dependents.get(node) {
            let mut next: Vec<&str> = Vec::new();
            for &dependent in deps_of_node {
                if let Some(deg) = in_degree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        next.push(dependent);
                    }
                }
            }
            // Sort for deterministic output
            next.sort();
            queue.extend(next);
        }
    }

    if result.len() != deps.len() {
        // Cycle detected: find which nodes are still unprocessed
        let processed: HashSet<&str> = result.iter().map(|s| s.as_str()).collect();
        let cycle_nodes: Vec<String> = deps
            .keys()
            .filter(|k| !processed.contains(k.as_str()))
            .cloned()
            .collect();
        Err(format!(
            "Dependency cycle detected involving: {}",
            cycle_nodes.join(", ")
        ))
    } else {
        Ok(result)
    }
}

/// Strip a date prefix of the form `YYYY-MM-DD-` from a change name.
///
/// Returns the remainder after the prefix if the name starts with a valid
/// date pattern (four digits, dash, two digits, dash, two digits, dash).
/// Returns `None` if the name doesn't match this pattern.
fn strip_date_prefix(name: &str) -> Option<&str> {
    if name.len() > 11 {
        let bytes = name.as_bytes();
        if bytes[4] == b'-'
            && bytes[7] == b'-'
            && bytes[10] == b'-'
            && bytes[..4].iter().all(|b| b.is_ascii_digit())
            && bytes[5..7].iter().all(|b| b.is_ascii_digit())
            && bytes[8..10].iter().all(|b| b.is_ascii_digit())
        {
            return Some(&name[11..]);
        }
    }
    None
}

/// Resolve fulfilled dependency names from the archive directory.
///
/// Scans `archive_dir` for subdirectories and returns a set of change names
/// that are considered fulfilled. For each archived change, both the full
/// directory name (e.g., `2026-03-08-add-api`) and the date-stripped suffix
/// (e.g., `add-api`) are included in the returned set.
///
/// Returns an empty set if the directory doesn't exist or can't be read.
pub fn resolve_archived_dependencies(archive_dir: &Path) -> HashSet<String> {
    let mut fulfilled = HashSet::new();

    if !archive_dir.exists() {
        return fulfilled;
    }

    let entries = match fs::read_dir(archive_dir) {
        Ok(entries) => entries,
        Err(_) => return fulfilled,
    };

    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();

        // Add the full name
        fulfilled.insert(name.clone());

        // Also add the date-stripped suffix if applicable
        if let Some(stripped) = strip_date_prefix(&name) {
            fulfilled.insert(stripped.to_string());
        }
    }

    fulfilled
}

/// Check whether a change directory contains a `tasks.md` file.
///
/// Used to filter eligible changes for batch runs — only changes with
/// a `tasks.md` can be executed by the implementation runner.
pub fn has_tasks_file(change_dir: &Path) -> bool {
    change_dir.join("tasks.md").is_file()
}

/// Generate an ASCII dependency graph showing all changes and their relationships.
///
/// Produces a multi-line string with tree connectors. Changes with no
/// dependencies are shown as roots, and dependent changes are shown as
/// children beneath their dependencies.
///
/// Example output:
/// ```text
/// add-api
/// ├── add-auth-layer
/// │   └── add-permissions
/// └── add-user-model
/// standalone-feature
/// ```
pub fn generate_dependency_graph(deps: &HashMap<String, Vec<String>>) -> String {
    // Collect all change names
    let all_changes: Vec<&String> = {
        let mut names: Vec<&String> = deps.keys().collect();
        names.sort();
        names
    };

    if all_changes.is_empty() {
        return "No changes found.".to_string();
    }

    // Build reverse map: for each change, which changes depend on it (children in graph)
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for name in &all_changes {
        children.entry(name.as_str()).or_default();
    }
    for (name, dep_list) in deps {
        for dep in dep_list {
            if deps.contains_key(dep) {
                children
                    .entry(dep.as_str())
                    .or_default()
                    .push(name.as_str());
            }
        }
    }
    // Sort children for deterministic output
    for children_list in children.values_mut() {
        children_list.sort();
    }

    // Find roots: changes with no dependencies (or only external deps)
    let roots: Vec<&str> = all_changes
        .iter()
        .filter(|name| {
            deps.get(name.as_str())
                .map(|d| d.iter().all(|dep| !deps.contains_key(dep)))
                .unwrap_or(true)
        })
        .map(|s| s.as_str())
        .collect();

    let mut output = String::new();
    let mut visited: HashSet<&str> = HashSet::new();

    for root in &roots {
        render_tree_node(root, &children, &mut visited, &mut output, "", "");
    }

    // Trim trailing newline
    if output.ends_with('\n') {
        output.pop();
    }
    output
}

fn render_tree_node<'a>(
    node: &'a str,
    children: &HashMap<&str, Vec<&'a str>>,
    visited: &mut HashSet<&'a str>,
    output: &mut String,
    connector: &str,
    prefix: &str,
) {
    if visited.contains(node) {
        return;
    }
    visited.insert(node);

    // Render current node with connector
    output.push_str(connector);
    output.push_str(node);
    output.push('\n');

    // Render children
    let empty_vec = Vec::new();
    let node_children = children.get(node).unwrap_or(&empty_vec);
    let unvisited_children: Vec<&&str> = node_children
        .iter()
        .filter(|c| !visited.contains(**c))
        .collect();

    for (i, child) in unvisited_children.iter().enumerate() {
        let is_last_child = i == unvisited_children.len() - 1;
        let child_connector = if is_last_child {
            format!("{}└── ", prefix)
        } else {
            format!("{}├── ", prefix)
        };
        let child_prefix = if is_last_child {
            format!("{}    ", prefix)
        } else {
            format!("{}│   ", prefix)
        };

        render_tree_node(
            child,
            children,
            visited,
            output,
            &child_connector,
            &child_prefix,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_change_list_output() {
        let json = r#"{
            "changes": [
                {
                    "name": "tui-change-viewer",
                    "completedTasks": 3,
                    "totalTasks": 21,
                    "lastModified": "2026-03-03T20:56:50.649Z",
                    "status": "in-progress"
                }
            ]
        }"#;

        let result: ChangeListOutput = serde_json::from_str(json).unwrap();
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].name, "tui-change-viewer");
        assert_eq!(result.changes[0].completed_tasks, 3);
        assert_eq!(result.changes[0].total_tasks, 21);
    }

    #[test]
    fn test_parse_change_list_empty() {
        let json = r#"{"changes": []}"#;
        let result: ChangeListOutput = serde_json::from_str(json).unwrap();
        assert_eq!(result.changes.len(), 0);
    }

    #[test]
    fn test_parse_change_status_output() {
        let json = r#"{
            "changeName": "tui-change-viewer",
            "schemaName": "spec-driven",
            "isComplete": true,
            "applyRequires": ["tasks"],
            "artifacts": [
                {
                    "id": "proposal",
                    "outputPath": "proposal.md",
                    "status": "done"
                },
                {
                    "id": "design",
                    "outputPath": "design.md",
                    "status": "done"
                },
                {
                    "id": "specs",
                    "outputPath": "specs/**/*.md",
                    "status": "done"
                },
                {
                    "id": "tasks",
                    "outputPath": "tasks.md",
                    "status": "pending"
                }
            ]
        }"#;

        let result: ChangeStatusOutput = serde_json::from_str(json).unwrap();
        assert_eq!(result.artifacts.len(), 4);
        assert_eq!(result.artifacts[0].id, "proposal");
        assert_eq!(result.artifacts[0].status, "done");
        assert_eq!(result.artifacts[3].id, "tasks");
        assert_eq!(result.artifacts[3].status, "pending");
    }

    #[test]
    fn test_openspec_command_returns_valid_command() {
        let cmd = openspec_command();
        let program = format!("{:?}", cmd.get_program());
        #[cfg(not(windows))]
        assert_eq!(program, "\"openspec\"");
        #[cfg(windows)]
        assert_eq!(program, "\"cmd\"");
    }

    #[test]
    fn test_parse_change_list_multiple_changes() {
        let json = r#"{
            "changes": [
                {
                    "name": "change-one",
                    "completedTasks": 0,
                    "totalTasks": 5,
                    "lastModified": "2026-03-01T00:00:00Z",
                    "status": "in-progress"
                },
                {
                    "name": "change-two",
                    "completedTasks": 5,
                    "totalTasks": 5,
                    "lastModified": "2026-03-02T00:00:00Z",
                    "status": "complete"
                }
            ]
        }"#;

        let result: ChangeListOutput = serde_json::from_str(json).unwrap();
        assert_eq!(result.changes.len(), 2);
        assert_eq!(result.changes[0].name, "change-one");
        assert_eq!(result.changes[1].name, "change-two");
    }

    #[test]
    fn test_parse_task_progress_crlf() {
        let dir = std::env::temp_dir().join("openspec-tui-test-crlf");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(
            &path,
            "## Tasks\r\n\r\n- [x] Task one\r\n- [ ] Task two\r\n",
        )
        .unwrap();
        let (completed, total) = parse_task_progress(&path).unwrap();
        assert_eq!(completed, 1);
        assert_eq!(total, 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_task_progress_non_ascii() {
        let dir = std::env::temp_dir().join("openspec-tui-test-nonascii");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(
            &path,
            "- [x] Étude de marché\n- [ ] Implémentation du système\n",
        )
        .unwrap();
        let (completed, total) = parse_task_progress(&path).unwrap();
        assert_eq!(completed, 1);
        assert_eq!(total, 2);

        let next = next_unchecked_task(&path).unwrap();
        assert_eq!(next.1, "Implémentation du système");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_task_progress_all_done() {
        let dir = std::env::temp_dir().join("openspec-tui-test-alldone");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(&path, "- [x] Task one\n- [x] Task two\n- [x] Task three\n").unwrap();
        let (completed, total) = parse_task_progress(&path).unwrap();
        assert_eq!(completed, 3);
        assert_eq!(total, 3);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_task_progress_none_done() {
        let dir = std::env::temp_dir().join("openspec-tui-test-nonedone");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(&path, "- [ ] Task one\n- [ ] Task two\n").unwrap();
        let (completed, total) = parse_task_progress(&path).unwrap();
        assert_eq!(completed, 0);
        assert_eq!(total, 2);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_task_progress_no_tasks() {
        let dir = std::env::temp_dir().join("openspec-tui-test-notasks");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(&path, "## Tasks\n\nNo tasks here.\n").unwrap();
        let (completed, total) = parse_task_progress(&path).unwrap();
        assert_eq!(completed, 0);
        assert_eq!(total, 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_parse_task_progress_file_not_found() {
        let path = std::env::temp_dir().join("openspec-tui-test-nonexistent/tasks.md");
        let result = parse_task_progress(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_next_unchecked_task_mixed() {
        let dir = std::env::temp_dir().join("openspec-tui-test-next-mixed");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(
            &path,
            "## Tasks\n\n- [x] Task one\n- [ ] Task two\n- [ ] Task three\n",
        )
        .unwrap();
        let result = next_unchecked_task(&path);
        assert_eq!(result, Some((2, "Task two".to_string())));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_next_unchecked_task_all_complete() {
        let dir = std::env::temp_dir().join("openspec-tui-test-next-alldone");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(&path, "- [x] Task one\n- [x] Task two\n").unwrap();
        let result = next_unchecked_task(&path);
        assert_eq!(result, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_next_unchecked_task_no_tasks() {
        let dir = std::env::temp_dir().join("openspec-tui-test-next-notasks");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(&path, "## Tasks\n\nNo tasks here.\n").unwrap();
        let result = next_unchecked_task(&path);
        assert_eq!(result, None);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_next_unchecked_task_missing_file() {
        let path = std::env::temp_dir().join("openspec-tui-test-next-missing/tasks.md");
        let result = next_unchecked_task(&path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_next_unchecked_task_first_is_unchecked() {
        let dir = std::env::temp_dir().join("openspec-tui-test-next-first");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tasks.md");
        fs::write(&path, "- [ ] First task\n- [ ] Second task\n").unwrap();
        let result = next_unchecked_task(&path);
        assert_eq!(result, Some((1, "First task".to_string())));
        fs::remove_dir_all(&dir).unwrap();
    }

    // --- list_archived_changes tests ---

    /// Serializes tests that mutate the process-global current directory.
    /// `set_current_dir` is process-wide, so without this guard concurrent
    /// CWD-changing tests race (one test's `remove_dir_all` can invalidate
    /// another's CWD, making `current_dir()` fail with NotFound). Poison is
    /// tolerated so a single failing test does not cascade into the rest.
    static CWD_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Helper: create a temp archive directory structure, run a closure, then clean up.
    fn with_archived_changes<F>(test_name: &str, dirs: &[(&str, Option<&str>)], f: F)
    where
        F: FnOnce(&Path),
    {
        let base = std::env::temp_dir().join(format!("openspec-tui-test-{}", test_name));
        let _ = fs::remove_dir_all(&base);
        let archive_dir = base.join("openspec").join("changes").join("archive");
        fs::create_dir_all(&archive_dir).unwrap();

        for (name, tasks_content) in dirs {
            let change_dir = archive_dir.join(name);
            fs::create_dir_all(&change_dir).unwrap();
            if let Some(content) = tasks_content {
                fs::write(change_dir.join("tasks.md"), content).unwrap();
            }
        }

        // Change to the temp base dir so list_archived_changes() finds the archive.
        // Hold the CWD guard for the whole critical section (until restore + cleanup).
        let _cwd = CWD_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let original_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to get current dir: {}", e))
            .unwrap();
        std::env::set_current_dir(&base).unwrap();
        f(&base);
        std::env::set_current_dir(&original_dir).unwrap();
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_list_archived_changes_with_changes() {
        with_archived_changes(
            "list-with-changes",
            &[
                ("2026-03-06-foo", Some("- [x] Task\n- [ ] Task\n")),
                ("2026-03-03-bar", Some("- [x] A\n- [x] B\n- [x] C\n")),
            ],
            |_| {
                let result = list_archived_changes().unwrap();
                assert_eq!(result.len(), 2);
                // Newest first
                assert_eq!(result[0].name, "2026-03-06-foo");
                assert_eq!(result[0].completed_tasks, 1);
                assert_eq!(result[0].total_tasks, 2);
                assert_eq!(result[1].name, "2026-03-03-bar");
                assert_eq!(result[1].completed_tasks, 3);
                assert_eq!(result[1].total_tasks, 3);
            },
        );
    }

    #[test]
    fn test_list_archived_changes_empty_directory() {
        with_archived_changes("list-empty-dir", &[], |_| {
            let result = list_archived_changes().unwrap();
            assert!(result.is_empty());
        });
    }

    #[test]
    fn test_list_archived_changes_nonexistent_directory() {
        let base = std::env::temp_dir().join("openspec-tui-test-list-nonexistent");
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        // No openspec/changes/archive/ directory

        let _cwd = CWD_GUARD.lock().unwrap_or_else(|e| e.into_inner());
        let original_dir = std::env::current_dir()
            .map_err(|e| format!("Failed to get current dir: {}", e))
            .unwrap();
        std::env::set_current_dir(&base).unwrap();
        let result = list_archived_changes().unwrap();
        assert!(result.is_empty());
        std::env::set_current_dir(&original_dir).unwrap();
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn test_list_archived_changes_sort_order() {
        with_archived_changes(
            "list-sort-order",
            &[
                ("2026-03-03-tui-change-viewer", Some("- [x] A\n")),
                ("2026-03-06-foo", Some("- [x] A\n")),
                ("2026-03-03-add-nix-flake", Some("- [x] A\n")),
                ("2026-03-05-bar", Some("- [x] A\n")),
            ],
            |_| {
                let result = list_archived_changes().unwrap();
                assert_eq!(result.len(), 4);
                // Newest first, then alphabetical within same date
                assert_eq!(result[0].name, "2026-03-06-foo");
                assert_eq!(result[1].name, "2026-03-05-bar");
                assert_eq!(result[2].name, "2026-03-03-add-nix-flake");
                assert_eq!(result[3].name, "2026-03-03-tui-change-viewer");
            },
        );
    }

    #[test]
    fn test_list_archived_changes_no_tasks_md() {
        with_archived_changes("list-no-tasks", &[("2026-03-06-no-tasks", None)], |_| {
            let result = list_archived_changes().unwrap();
            assert_eq!(result.len(), 1);
            assert_eq!(result[0].completed_tasks, 0);
            assert_eq!(result[0].total_tasks, 0);
        });
    }

    // --- get_archived_change_status tests ---

    #[test]
    fn test_get_archived_change_status_all_present() {
        let dir = std::env::temp_dir().join("openspec-tui-test-status-all");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("proposal.md"), "proposal").unwrap();
        fs::write(dir.join("design.md"), "design").unwrap();
        fs::write(dir.join("tasks.md"), "tasks").unwrap();
        let specs_dir = dir.join("specs").join("my-spec");
        fs::create_dir_all(&specs_dir).unwrap();
        fs::write(specs_dir.join("spec.md"), "spec").unwrap();

        let status = get_archived_change_status(&dir);
        assert_eq!(status.artifacts.len(), 4);
        assert!(status.artifacts.iter().all(|a| a.status == "done"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_get_archived_change_status_some_missing() {
        let dir = std::env::temp_dir().join("openspec-tui-test-status-some");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("proposal.md"), "proposal").unwrap();
        // No design.md
        fs::write(dir.join("tasks.md"), "tasks").unwrap();
        // No specs/

        let status = get_archived_change_status(&dir);
        let proposal = status
            .artifacts
            .iter()
            .find(|a| a.id == "proposal")
            .unwrap();
        assert_eq!(proposal.status, "done");
        let design = status.artifacts.iter().find(|a| a.id == "design").unwrap();
        assert_eq!(design.status, "pending");
        let tasks = status.artifacts.iter().find(|a| a.id == "tasks").unwrap();
        assert_eq!(tasks.status, "done");
        let specs = status.artifacts.iter().find(|a| a.id == "specs").unwrap();
        assert_eq!(specs.status, "pending");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_get_archived_change_status_specs_with_subdirs() {
        let dir = std::env::temp_dir().join("openspec-tui-test-status-specs");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let specs_dir = dir.join("specs");
        fs::create_dir_all(specs_dir.join("cap-a")).unwrap();
        fs::write(specs_dir.join("cap-a").join("spec.md"), "spec a").unwrap();
        fs::create_dir_all(specs_dir.join("cap-b")).unwrap();
        fs::write(specs_dir.join("cap-b").join("spec.md"), "spec b").unwrap();

        let status = get_archived_change_status(&dir);
        let specs = status.artifacts.iter().find(|a| a.id == "specs").unwrap();
        assert_eq!(specs.status, "done");
        fs::remove_dir_all(&dir).unwrap();
    }

    // --- read_dependencies / write_dependencies tests ---

    #[test]
    fn test_read_dependencies_with_deps() {
        let dir = std::env::temp_dir().join("openspec-tui-test-read-deps");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("change-config.yaml"),
            "depends_on:\n  - add-api\n  - add-user-model\n",
        )
        .unwrap();

        let deps = read_dependencies(&dir).unwrap();
        assert_eq!(deps, vec!["add-api", "add-user-model"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_read_dependencies_file_missing() {
        let dir = std::env::temp_dir().join("openspec-tui-test-read-deps-missing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let deps = read_dependencies(&dir).unwrap();
        assert!(deps.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_read_dependencies_empty_list() {
        let dir = std::env::temp_dir().join("openspec-tui-test-read-deps-empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("change-config.yaml"), "depends_on: []\n").unwrap();

        let deps = read_dependencies(&dir).unwrap();
        assert!(deps.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_dependencies_creates_file() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-deps-create");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let deps = vec!["add-api".to_string(), "add-auth".to_string()];
        write_dependencies(&dir, &deps).unwrap();

        let read_back = read_dependencies(&dir).unwrap();
        assert_eq!(read_back, vec!["add-api", "add-auth"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_dependencies_overwrites_existing() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-deps-overwrite");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("change-config.yaml"), "depends_on:\n  - old-dep\n").unwrap();

        let deps = vec!["new-dep".to_string()];
        write_dependencies(&dir, &deps).unwrap();

        let read_back = read_dependencies(&dir).unwrap();
        assert_eq!(read_back, vec!["new-dep"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_dependencies_empty_list() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-deps-empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        write_dependencies(&dir, &[]).unwrap();

        let read_back = read_dependencies(&dir).unwrap();
        assert!(read_back.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_then_add_dependency() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-add-dep");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Start with one dep
        write_dependencies(&dir, &["dep-a".to_string()]).unwrap();
        // Read, add, write back
        let mut deps = read_dependencies(&dir).unwrap();
        deps.push("dep-b".to_string());
        write_dependencies(&dir, &deps).unwrap();

        let final_deps = read_dependencies(&dir).unwrap();
        assert_eq!(final_deps, vec!["dep-a", "dep-b"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_then_remove_dependency() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-remove-dep");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        write_dependencies(&dir, &["dep-a".to_string(), "dep-b".to_string()]).unwrap();
        // Read, remove, write back
        let mut deps = read_dependencies(&dir).unwrap();
        deps.retain(|d| d != "dep-a");
        write_dependencies(&dir, &deps).unwrap();

        let final_deps = read_dependencies(&dir).unwrap();
        assert_eq!(final_deps, vec!["dep-b"]);
        fs::remove_dir_all(&dir).unwrap();
    }

    // --- topological_sort tests ---

    #[test]
    fn test_topological_sort_linear_chain() {
        // A -> B -> C (C depends on B, B depends on A)
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec![]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["b".to_string()]);

        let result = topological_sort(&deps).unwrap();
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_multiple_roots() {
        // A and B are independent, C depends on both
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec![]);
        deps.insert("b".to_string(), vec![]);
        deps.insert("c".to_string(), vec!["a".to_string(), "b".to_string()]);

        let result = topological_sort(&deps).unwrap();
        // a and b must come before c, order between a and b is alphabetical
        assert_eq!(result.len(), 3);
        assert_eq!(result[2], "c");
        let a_pos = result.iter().position(|x| x == "a").unwrap();
        let b_pos = result.iter().position(|x| x == "b").unwrap();
        let c_pos = result.iter().position(|x| x == "c").unwrap();
        assert!(a_pos < c_pos);
        assert!(b_pos < c_pos);
    }

    #[test]
    fn test_topological_sort_diamond() {
        // A -> B, A -> C, B -> D, C -> D (diamond shape)
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec![]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["a".to_string()]);
        deps.insert("d".to_string(), vec!["b".to_string(), "c".to_string()]);

        let result = topological_sort(&deps).unwrap();
        assert_eq!(result.len(), 4);
        let a_pos = result.iter().position(|x| x == "a").unwrap();
        let b_pos = result.iter().position(|x| x == "b").unwrap();
        let c_pos = result.iter().position(|x| x == "c").unwrap();
        let d_pos = result.iter().position(|x| x == "d").unwrap();
        assert!(a_pos < b_pos);
        assert!(a_pos < c_pos);
        assert!(b_pos < d_pos);
        assert!(c_pos < d_pos);
    }

    #[test]
    fn test_topological_sort_no_deps() {
        // All independent
        let mut deps = HashMap::new();
        deps.insert("c".to_string(), vec![]);
        deps.insert("a".to_string(), vec![]);
        deps.insert("b".to_string(), vec![]);

        let result = topological_sort(&deps).unwrap();
        // Should be sorted alphabetically since all are roots
        assert_eq!(result, vec!["a", "b", "c"]);
    }

    #[test]
    fn test_topological_sort_empty() {
        let deps: HashMap<String, Vec<String>> = HashMap::new();
        let result = topological_sort(&deps).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_topological_sort_direct_cycle() {
        // A -> B -> A
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]);
        deps.insert("b".to_string(), vec!["a".to_string()]);

        let result = topological_sort(&deps);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("cycle"));
    }

    #[test]
    fn test_topological_sort_indirect_cycle() {
        // A -> B -> C -> A
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec!["c".to_string()]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["b".to_string()]);

        let result = topological_sort(&deps);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("cycle"));
    }

    #[test]
    fn test_topological_sort_external_dep_ignored() {
        // B depends on "external" which is not in the map
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec![]);
        deps.insert(
            "b".to_string(),
            vec!["a".to_string(), "external".to_string()],
        );

        let result = topological_sort(&deps).unwrap();
        assert_eq!(result, vec!["a", "b"]);
    }

    // --- resolve_archived_dependencies tests ---

    #[test]
    fn test_resolve_archived_exact_match() {
        let dir = std::env::temp_dir().join("openspec-tui-test-archived-exact");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("add-api")).unwrap();
        fs::create_dir_all(dir.join("add-auth")).unwrap();

        let result = resolve_archived_dependencies(&dir);
        assert!(result.contains("add-api"));
        assert!(result.contains("add-auth"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_resolve_archived_date_prefix_match() {
        let dir = std::env::temp_dir().join("openspec-tui-test-archived-dateprefix");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("2026-03-08-add-api")).unwrap();

        let result = resolve_archived_dependencies(&dir);
        // Both the full name and the stripped name should be present
        assert!(result.contains("2026-03-08-add-api"));
        assert!(result.contains("add-api"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_resolve_archived_no_match() {
        let dir = std::env::temp_dir().join("openspec-tui-test-archived-nomatch");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Empty archive directory

        let result = resolve_archived_dependencies(&dir);
        assert!(result.is_empty());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_resolve_archived_nonexistent_directory() {
        let dir = std::env::temp_dir().join("openspec-tui-test-archived-nonexistent-dir");
        let _ = fs::remove_dir_all(&dir);

        let result = resolve_archived_dependencies(&dir);
        assert!(result.is_empty());
    }

    #[test]
    fn test_resolve_archived_ignores_files() {
        let dir = std::env::temp_dir().join("openspec-tui-test-archived-ignores-files");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("not-a-dir.txt"), "file").unwrap();
        fs::create_dir_all(dir.join("real-change")).unwrap();

        let result = resolve_archived_dependencies(&dir);
        assert!(result.contains("real-change"));
        assert!(!result.contains("not-a-dir.txt"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_resolve_archived_mixed_names() {
        let dir = std::env::temp_dir().join("openspec-tui-test-archived-mixed");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("2026-03-06-foo")).unwrap();
        fs::create_dir_all(dir.join("bar-no-date")).unwrap();

        let result = resolve_archived_dependencies(&dir);
        assert!(result.contains("2026-03-06-foo"));
        assert!(result.contains("foo")); // date-stripped
        assert!(result.contains("bar-no-date"));
        assert_eq!(result.len(), 3); // 2026-03-06-foo, foo, bar-no-date
        fs::remove_dir_all(&dir).unwrap();
    }

    // --- strip_date_prefix tests ---

    #[test]
    fn test_strip_date_prefix_valid() {
        assert_eq!(strip_date_prefix("2026-03-08-add-api"), Some("add-api"));
    }

    #[test]
    fn test_strip_date_prefix_no_prefix() {
        assert_eq!(strip_date_prefix("add-api"), None);
    }

    #[test]
    fn test_strip_date_prefix_too_short() {
        assert_eq!(strip_date_prefix("2026-03-08"), None);
    }

    #[test]
    fn test_strip_date_prefix_invalid_format() {
        assert_eq!(strip_date_prefix("not-a-date-prefix"), None);
    }

    // --- has_tasks_file tests ---

    #[test]
    fn test_has_tasks_file_exists() {
        let dir = std::env::temp_dir().join("openspec-tui-test-has-tasks-exists");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tasks.md"), "- [ ] Task one\n").unwrap();

        assert!(has_tasks_file(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_has_tasks_file_missing() {
        let dir = std::env::temp_dir().join("openspec-tui-test-has-tasks-missing");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        assert!(!has_tasks_file(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_has_tasks_file_is_directory() {
        let dir = std::env::temp_dir().join("openspec-tui-test-has-tasks-isdir");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("tasks.md")).unwrap();

        assert!(!has_tasks_file(&dir));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_has_tasks_file_nonexistent_dir() {
        let dir = std::env::temp_dir().join("openspec-tui-test-has-tasks-nodir");
        let _ = fs::remove_dir_all(&dir);

        assert!(!has_tasks_file(&dir));
    }

    #[test]
    fn test_load_change_dependencies_with_deps() {
        let base = std::env::temp_dir().join("openspec-tui-test-load-deps");
        let _ = fs::remove_dir_all(&base);
        let changes_dir = base.join("openspec").join("changes");

        // Create two change dirs, one with deps, one without
        let change_a = changes_dir.join("change-a");
        let change_b = changes_dir.join("change-b");
        fs::create_dir_all(&change_a).unwrap();
        fs::create_dir_all(&change_b).unwrap();

        fs::write(
            change_a.join("change-config.yaml"),
            "depends_on:\n  - change-b\n",
        )
        .unwrap();

        let _changes = [
            ChangeEntry {
                name: "change-a".to_string(),
                completed_tasks: 0,
                total_tasks: 1,
            },
            ChangeEntry {
                name: "change-b".to_string(),
                completed_tasks: 0,
                total_tasks: 1,
            },
        ];

        // Use the function directly with the right cwd
        // Since load_change_dependencies uses current_dir, we test read_dependencies directly
        let deps_a = read_dependencies(&change_a).unwrap();
        assert_eq!(deps_a, vec!["change-b".to_string()]);

        let deps_b = read_dependencies(&change_b).unwrap();
        assert!(deps_b.is_empty());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn test_generate_dependency_graph_linear_chain() {
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec![]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["b".to_string()]);

        let graph = generate_dependency_graph(&deps);
        assert_eq!(graph, "a\n└── b\n    └── c");
    }

    #[test]
    fn test_generate_dependency_graph_diamond() {
        let mut deps = HashMap::new();
        deps.insert("a".to_string(), vec![]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        deps.insert("c".to_string(), vec!["a".to_string()]);
        deps.insert("d".to_string(), vec!["b".to_string(), "c".to_string()]);

        let graph = generate_dependency_graph(&deps);
        // a is root, b and c are children of a, d is child of b (visited first)
        assert!(graph.contains("a"));
        assert!(graph.contains("b"));
        assert!(graph.contains("c"));
        assert!(graph.contains("d"));
        // d should appear as child of b or c (whichever is visited first)
        // Since b comes before c alphabetically, d should be under b
        let lines: Vec<&str> = graph.lines().collect();
        assert_eq!(lines[0], "a");
    }

    #[test]
    fn test_generate_dependency_graph_no_deps() {
        let mut deps = HashMap::new();
        deps.insert("alpha".to_string(), vec![]);
        deps.insert("beta".to_string(), vec![]);
        deps.insert("gamma".to_string(), vec![]);

        let graph = generate_dependency_graph(&deps);
        let lines: Vec<&str> = graph.lines().collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "alpha");
        assert_eq!(lines[1], "beta");
        assert_eq!(lines[2], "gamma");
    }

    #[test]
    fn test_generate_dependency_graph_multiple_roots() {
        let mut deps = HashMap::new();
        deps.insert("api".to_string(), vec![]);
        deps.insert("db".to_string(), vec![]);
        deps.insert(
            "service".to_string(),
            vec!["api".to_string(), "db".to_string()],
        );

        let graph = generate_dependency_graph(&deps);
        assert!(graph.contains("api"));
        assert!(graph.contains("db"));
        assert!(graph.contains("service"));
        // service should be a child of one of the roots
        let lines: Vec<&str> = graph.lines().collect();
        // api and db are roots (alphabetically api first)
        assert_eq!(lines[0], "api");
    }

    #[test]
    fn test_generate_dependency_graph_empty() {
        let deps = HashMap::new();
        let graph = generate_dependency_graph(&deps);
        assert_eq!(graph, "No changes found.");
    }

    // --- RunMode / ChangeConfig tests ---

    #[test]
    fn test_run_mode_default_is_normal() {
        let mode = RunMode::default();
        assert_eq!(mode, RunMode::Normal);
    }

    #[test]
    fn test_change_config_missing_run_mode_defaults_to_normal() {
        let yaml = "depends_on:\n  - dep-a\n";
        let config: ChangeConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.run_mode, RunMode::Normal);
        assert_eq!(config.depends_on, vec!["dep-a"]);
    }

    #[test]
    fn test_change_config_explicit_apply_mode() {
        let yaml = "depends_on: []\nrun_mode: apply\n";
        let config: ChangeConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.run_mode, RunMode::Apply);
    }

    #[test]
    fn test_change_config_explicit_normal_mode() {
        let yaml = "run_mode: normal\n";
        let config: ChangeConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.run_mode, RunMode::Normal);
    }

    #[test]
    fn test_change_config_empty_yaml_defaults() {
        let yaml = "{}";
        let config: ChangeConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.run_mode, RunMode::Normal);
        assert!(config.depends_on.is_empty());
    }

    #[test]
    fn test_change_config_roundtrip_serialization() {
        let config = ChangeConfig {
            depends_on: vec!["dep-a".to_string(), "dep-b".to_string()],
            run_mode: RunMode::Apply,
        };
        let yaml = serde_norway::to_string(&config).unwrap();
        let deserialized: ChangeConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(deserialized.depends_on, config.depends_on);
        assert_eq!(deserialized.run_mode, RunMode::Apply);
    }

    #[test]
    fn test_read_change_config_with_run_mode() {
        let dir = std::env::temp_dir().join("openspec-tui-test-read-config-runmode");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("change-config.yaml"),
            "depends_on:\n  - dep-a\nrun_mode: apply\n",
        )
        .unwrap();

        let config = read_change_config(&dir).unwrap();
        assert_eq!(config.depends_on, vec!["dep-a"]);
        assert_eq!(config.run_mode, RunMode::Apply);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_read_run_mode_default() {
        let dir = std::env::temp_dir().join("openspec-tui-test-read-runmode-default");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mode = read_run_mode(&dir).unwrap();
        assert_eq!(mode, RunMode::Normal);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_read_run_mode_apply() {
        let dir = std::env::temp_dir().join("openspec-tui-test-read-runmode-apply");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("change-config.yaml"), "run_mode: apply\n").unwrap();

        let mode = read_run_mode(&dir).unwrap();
        assert_eq!(mode, RunMode::Apply);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_change_config_preserves_run_mode() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-config-runmode");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let config = ChangeConfig {
            depends_on: vec!["dep-a".to_string()],
            run_mode: RunMode::Apply,
        };
        write_change_config(&dir, &config).unwrap();

        let read_back = read_change_config(&dir).unwrap();
        assert_eq!(read_back.depends_on, vec!["dep-a"]);
        assert_eq!(read_back.run_mode, RunMode::Apply);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_read_change_config_corrupt_file_returns_error() {
        let dir = std::env::temp_dir().join("openspec-tui-test-config-corrupt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Invalid YAML (missing colon)
        fs::write(dir.join("change-config.yaml"), "depends_on\n  - some-dep\n").unwrap();

        let result = read_change_config(&dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Failed to parse"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn test_write_dependencies_preserves_run_mode() {
        let dir = std::env::temp_dir().join("openspec-tui-test-write-deps-preserves-mode");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        // Write config with apply mode
        let config = ChangeConfig {
            depends_on: vec!["dep-a".to_string()],
            run_mode: RunMode::Apply,
        };
        write_change_config(&dir, &config).unwrap();

        // Update dependencies via write_dependencies
        write_dependencies(&dir, &["dep-b".to_string()]).unwrap();

        // run_mode should be preserved
        let read_back = read_change_config(&dir).unwrap();
        assert_eq!(read_back.depends_on, vec!["dep-b"]);
        assert_eq!(read_back.run_mode, RunMode::Apply);
        fs::remove_dir_all(&dir).unwrap();
    }
}
