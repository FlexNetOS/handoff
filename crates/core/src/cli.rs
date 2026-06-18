use crate::fs_utils::{ensure_dir, write_string_preserving_existing};
use crate::manifest::write_manifest;
use crate::planner::{
    generate_plan_from_paths, inventory_json, inventory_markdown, task_markdown, write_task,
};
use crate::scanner::scan_repo;
use crate::templates;
use crate::validation::write_validation_report;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn run<I, S>(args: I) -> Result<(), String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args = args.into_iter().map(Into::into).collect::<Vec<_>>();
    let command = args.get(1).map(String::as_str).unwrap_or("help");

    match command {
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        "init" => cmd_init(&args[2..]),
        "scan" => cmd_scan(&args[2..]),
        "plan" => cmd_plan(&args[2..]),
        "task" => cmd_task(&args[2..]),
        "validate" => cmd_validate(&args[2..]),
        "manifest" => cmd_manifest(&args[2..]),
        "github" => cmd_github(&args[2..]),
        other => Err(format!("unknown command `{other}`. Run `idd help`.")),
    }
}

fn cmd_init(args: &[String]) -> Result<(), String> {
    let target = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    ensure_dir(&target).map_err(|e| format!("failed to create target: {e}"))?;
    ensure_dir(&target.join("AI_MERGE/07_tasks"))
        .map_err(|e| format!("failed to create AI_MERGE: {e}"))?;
    ensure_dir(&target.join(".idd")).map_err(|e| format!("failed to create .idd: {e}"))?;
    ensure_dir(&target.join(".github/workflows"))
        .map_err(|e| format!("failed to create workflows: {e}"))?;
    ensure_dir(&target.join(".github/ISSUE_TEMPLATE"))
        .map_err(|e| format!("failed to create issue templates: {e}"))?;

    write_template(&target.join("AGENTS.md"), templates::AGENTS_MD)?;
    write_template(
        &target.join(".github/copilot-instructions.md"),
        templates::COPILOT_INSTRUCTIONS,
    )?;
    write_template(
        &target.join(".github/pull_request_template.md"),
        templates::PR_TEMPLATE,
    )?;
    write_template(
        &target.join(".github/ISSUE_TEMPLATE/idd-task.yml"),
        templates::ISSUE_TEMPLATE,
    )?;
    write_template(&target.join("SECURITY.md"), templates::SECURITY_MD)?;
    write_template(&target.join(".idd/LOCK.md"), templates::LOCK_TEMPLATE)?;
    write_template(
        &target.join(".env.schema.example.json"),
        templates::ENV_SCHEMA_EXAMPLE,
    )?;
    write_template(
        &target.join(".github/workflows/idd-ci.yml"),
        templates::GITHUB_ACTIONS_CI,
    )?;
    write_template(
        &target.join("AI_MERGE/04_merge_plan.md"),
        "# Merge Plan\n\nRun `idd plan --repo-a <path> --repo-b <path> --out .` to generate a concrete plan.\n",
    )?;
    write_template(
        &target.join("AI_MERGE/03_env_and_secret_contracts.md"),
        "# Environment and Secrets Contract\n\nRun `idd plan` to generate this from actual repositories.\n",
    )?;
    write_template(
        &target.join("AI_MERGE/08_agent_queue.md"),
        templates::AGENT_QUEUE,
    )?;
    write_template(
        &target.join("AI_MERGE/09_github_execution.md"),
        templates::GITHUB_EXECUTION,
    )?;
    write_template(
        &target.join("AI_MERGE/10_parity_test_plan.md"),
        templates::PARITY_TEST_PLAN,
    )?;
    write_template(
        &target.join("AI_MERGE/11_provider_matrix.md"),
        templates::PROVIDER_MATRIX,
    )?;
    write_manifest(&target, target.join(".idd/MANIFEST.tsv"))?;
    println!("initialized IDD workspace at {}", target.display());
    Ok(())
}

fn cmd_scan(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let repo = required(&flags, "repo")?;
    let out = flags.get("out").map(PathBuf::from);
    let format = flags.get("format").map(String::as_str).unwrap_or("md");
    let inv = scan_repo(repo)?;
    let rendered = match format {
        "json" => inventory_json(&inv),
        "md" | "markdown" => inventory_markdown(&inv),
        other => return Err(format!("unsupported scan format `{other}`; use md or json")),
    };
    if let Some(out) = out {
        write_template(&out, &rendered)?;
        println!("wrote inventory to {}", out.display());
    } else {
        print!("{rendered}");
    }
    Ok(())
}

fn cmd_plan(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let repo_a = required(&flags, "repo-a")?;
    let repo_b = required(&flags, "repo-b")?;
    let out = flags.get("out").map(String::as_str).unwrap_or(".");
    let name = flags
        .get("name")
        .map(String::as_str)
        .unwrap_or("repo-unification");
    generate_plan_from_paths(repo_a, repo_b, out, name)?;
    println!("generated IDD merge workspace at {out}");
    Ok(())
}

fn cmd_task(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let out = flags
        .get("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("AI_MERGE/07_tasks"));
    let title = required(&flags, "title")?;
    let kind = flags.get("kind").map(String::as_str).unwrap_or("general");
    ensure_dir(&out).map_err(|e| format!("failed to create task directory: {e}"))?;
    write_task(&out, title, kind)?;
    println!("wrote task `{}` to {}", title, out.display());
    Ok(())
}

fn cmd_validate(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let workspace = flags.get("workspace").map(String::as_str).unwrap_or(".");
    let report = flags
        .get("report")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(workspace).join("AI_MERGE/validation_report.md"));
    let findings = write_validation_report(workspace, &report)?;
    let critical = findings
        .iter()
        .filter(|f| f.severity.to_string() == "critical")
        .count();
    let warnings = findings
        .iter()
        .filter(|f| f.severity.to_string() == "warning")
        .count();
    println!(
        "validation complete: {} critical, {} warning; report: {}",
        critical,
        warnings,
        report.display()
    );
    if critical > 0 {
        return Err("critical validation findings detected".to_string());
    }
    Ok(())
}

fn cmd_manifest(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let workspace = flags.get("workspace").map(String::as_str).unwrap_or(".");
    let out = flags
        .get("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(workspace).join(".idd/MANIFEST.tsv"));
    let entries = write_manifest(workspace, &out)?;
    println!(
        "wrote {} manifest entries to {}",
        entries.len(),
        out.display()
    );
    Ok(())
}

fn cmd_github(args: &[String]) -> Result<(), String> {
    let flags = parse_flags(args);
    let workspace = flags
        .get("workspace")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    ensure_dir(&workspace.join(".github/ISSUE_TEMPLATE"))
        .map_err(|e| format!("failed to create .github templates: {e}"))?;
    ensure_dir(&workspace.join(".github/workflows"))
        .map_err(|e| format!("failed to create workflows: {e}"))?;
    write_template(
        &workspace.join(".github/copilot-instructions.md"),
        templates::COPILOT_INSTRUCTIONS,
    )?;
    write_template(
        &workspace.join(".github/pull_request_template.md"),
        templates::PR_TEMPLATE,
    )?;
    write_template(
        &workspace.join(".github/ISSUE_TEMPLATE/idd-task.yml"),
        templates::ISSUE_TEMPLATE,
    )?;
    write_template(
        &workspace.join(".github/workflows/idd-ci.yml"),
        templates::GITHUB_ACTIONS_CI,
    )?;
    write_template(&workspace.join("SECURITY.md"), templates::SECURITY_MD)?;
    write_manifest(&workspace, workspace.join(".idd/MANIFEST.tsv"))?;
    println!("wrote GitHub agent templates to {}", workspace.display());
    Ok(())
}

fn parse_flags(args: &[String]) -> BTreeMap<String, String> {
    let mut flags = BTreeMap::new();
    let mut i = 0;
    while i < args.len() {
        let item = &args[i];
        if let Some(body) = item.strip_prefix("--") {
            if let Some((name, value)) = body.split_once('=') {
                flags.insert(name.to_string(), value.to_string());
                i += 1;
                continue;
            }
            if let Some(value) = args.get(i + 1) {
                if !value.starts_with("--") {
                    flags.insert(body.to_string(), value.to_string());
                    i += 2;
                    continue;
                }
            }
            flags.insert(body.to_string(), "true".to_string());
        }
        i += 1;
    }
    flags
}

fn required<'a>(flags: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    flags
        .get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("missing required flag --{key}"))
}

fn write_template(path: &Path, content: &str) -> Result<(), String> {
    write_string_preserving_existing(path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn print_help() {
    println!(
        "intent-driven-development (idd)

USAGE:
  idd init [path]
  idd scan --repo <path> [--out <file>] [--format md|json]
  idd plan --repo-a <path> --repo-b <path> --out <workspace> [--name <name>]
  idd task --title <title> [--kind <kind>] [--out AI_MERGE/07_tasks]
  idd validate [--workspace <path>] [--report <file>]
  idd manifest [--workspace <path>] [--out .idd/MANIFEST.tsv]
  idd github [--workspace <path>]

CORE IDEA:
  Convert repo unification into explicit intent, contracts, small tasks, CI gates,
  GitHub-native agent templates, and audit files that cloud or local AI agents can safely execute.

EXAMPLE:
  idd plan --repo-a ../env-manager --repo-b ../secrets-manager --out ./integration --name env-secrets-unification
"
    );
}

#[allow(dead_code)]
fn _task_preview(title: &str, kind: &str) -> String {
    task_markdown(title, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_flags_basic() {
        let args = vec![
            "--repo".to_string(),
            "path/to/repo".to_string(),
            "--format=json".to_string(),
        ];
        let flags = parse_flags(&args);
        assert_eq!(flags.get("repo").unwrap(), "path/to/repo");
        assert_eq!(flags.get("format").unwrap(), "json");
    }

    #[test]
    fn test_parse_flags_boolean() {
        let args = vec!["--dry-run".to_string()];
        let flags = parse_flags(&args);
        assert_eq!(flags.get("dry-run").unwrap(), "true");
    }

    #[test]
    fn test_required_flag_missing() {
        let flags = BTreeMap::new();
        let result = required(&flags, "missing");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "missing required flag --missing");
    }

    #[test]
    fn test_cmd_init_executes() {
        let tmp = tempdir().unwrap();
        let target = tmp.path().to_string_lossy().to_string();
        cmd_init(&[target]).unwrap();
        assert!(tmp.path().join("AGENTS.md").exists());
        assert!(tmp.path().join(".idd/LOCK.md").exists());
        assert!(tmp.path().join(".idd/MANIFEST.tsv").exists());
    }

    #[test]
    fn test_cmd_task_creates_file() {
        let tmp = tempdir().unwrap();
        let out = tmp.path().to_string_lossy().to_string();
        let args = vec![
            "--title".to_string(),
            "My Task".to_string(),
            "--out".to_string(),
            out.clone(),
        ];
        cmd_task(&args).unwrap();
        assert!(tmp.path().join("my-task.md").exists());
    }

    #[test]
    fn test_cmd_scan_requires_repo() {
        let result = cmd_scan(&[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("missing required flag --repo"));
    }
}
