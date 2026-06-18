use crate::fs_utils::{
    read_to_string_lossy, relative_path, stable_walk, write_string_preserving_existing,
};
use crate::model::{FindingSeverity, ValidationFinding};
use std::path::Path;

const MAX_SCAN_BYTES: u64 = 512 * 1024;

pub fn validate_workspace(path: impl AsRef<Path>) -> Result<Vec<ValidationFinding>, String> {
    let root = path.as_ref();
    if !root.exists() || !root.is_dir() {
        return Err(format!(
            "workspace path is not a directory: {}",
            root.display()
        ));
    }

    let mut findings = Vec::new();
    require_file(root, "AGENTS.md", &mut findings);
    require_file(root, ".github/copilot-instructions.md", &mut findings);
    require_file(root, ".github/pull_request_template.md", &mut findings);
    require_file(root, ".github/ISSUE_TEMPLATE/idd-task.yml", &mut findings);
    require_file(root, ".idd/LOCK.md", &mut findings);
    require_file(root, ".idd/MANIFEST.tsv", &mut findings);
    require_file(root, "AI_MERGE/04_merge_plan.md", &mut findings);
    require_file(
        root,
        "AI_MERGE/03_env_and_secret_contracts.md",
        &mut findings,
    );
    require_file(root, "AI_MERGE/08_agent_queue.md", &mut findings);
    require_file(root, "AI_MERGE/10_parity_test_plan.md", &mut findings);

    for abs in stable_walk(root).map_err(|e| format!("walk failed: {e}"))? {
        let rel = relative_path(root, &abs);
        if rel.contains(".git/") || rel.starts_with("target/") {
            continue;
        }
        flag_committed_env_file(&rel, &mut findings);
        let content = read_to_string_lossy(&abs, MAX_SCAN_BYTES).unwrap_or_default();
        if content.is_empty() {
            continue;
        }
        scan_secret_patterns(&rel, &content, &mut findings);
        if is_github_workflow(&rel) {
            scan_workflow_risks(&rel, &content, &mut findings);
        }
    }

    findings.sort_by(|a, b| b.severity.cmp(&a.severity).then(a.file.cmp(&b.file)));
    Ok(findings)
}

pub fn write_validation_report(
    workspace: impl AsRef<Path>,
    report_path: impl AsRef<Path>,
) -> Result<Vec<ValidationFinding>, String> {
    let findings = validate_workspace(workspace)?;
    let mut out =
        String::from("# IDD Validation Report\n\n| Severity | File | Finding |\n|---|---|---|\n");
    if findings.is_empty() {
        out.push_str("| info | _workspace_ | No findings |\n");
    } else {
        for finding in &findings {
            out.push_str(&format!(
                "| {} | `{}` | {} |\n",
                finding.severity, finding.file, finding.message
            ));
        }
    }
    write_string_preserving_existing(report_path.as_ref(), &out)
        .map_err(|e| format!("failed to write validation report: {e}"))?;
    Ok(findings)
}

fn require_file(root: &Path, rel: &str, findings: &mut Vec<ValidationFinding>) {
    if !root.join(rel).exists() {
        findings.push(ValidationFinding {
            severity: FindingSeverity::Warning,
            file: rel.to_string(),
            message: "required IDD/GitHub control-plane file is missing".to_string(),
        });
    }
}

fn flag_committed_env_file(file: &str, findings: &mut Vec<ValidationFinding>) {
    let lower = file.to_ascii_lowercase();
    let name = Path::new(file)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file)
        .to_ascii_lowercase();

    let allowed = name == ".env.example"
        || name == "env.example"
        || name == ".env.schema.example.json"
        || lower.ends_with(".sample")
        || lower.ends_with(".template");

    if name.starts_with(".env") && !allowed {
        findings.push(ValidationFinding {
            severity: FindingSeverity::Critical,
            file: file.to_string(),
            message: "committed dotenv file detected; keep real .env files local or encrypted"
                .to_string(),
        });
    }
}

fn scan_secret_patterns(file: &str, content: &str, findings: &mut Vec<ValidationFinding>) {
    let lower_file = file.to_ascii_lowercase();
    let is_allowed_example = lower_file.ends_with(".env.example")
        || lower_file.ends_with("env.schema.example.json")
        || lower_file.contains("/examples/")
        || lower_file.starts_with("examples/")
        || lower_file.ends_with(".md");

    for (line_no, line) in content.lines().enumerate() {
        let lower = line.to_ascii_lowercase();
        let critical = [
            ["-----begin ", "private key-----"].concat(),
            ["aws_secret", "_access_key="].concat(),
            ["gh", "p_"].concat(),
            ["github", "_pat_"].concat(),
            ["xo", "xb-"].concat(),
            ["sk", "-proj-"].concat(),
            ["sk", "-live-"].concat(),
            ["-----begin open", "ssh private key-----"].concat(),
        ];
        if critical
            .iter()
            .any(|needle| lower.contains(needle.as_str()))
            && !is_allowed_example
        {
            findings.push(ValidationFinding {
                severity: FindingSeverity::Critical,
                file: file.to_string(),
                message: format!("possible committed secret near line {}", line_no + 1),
            });
        }

        let warning = [
            "=drowssap".chars().rev().collect::<String>(),
            "=yek_ipa".chars().rev().collect::<String>(),
            "=yekipa".chars().rev().collect::<String>(),
            "=terces".chars().rev().collect::<String>(),
            "=nekot".chars().rev().collect::<String>(),
            "=terces_tneilc".chars().rev().collect::<String>(),
        ];
        if warning.iter().any(|needle| lower.contains(needle.as_str())) && !is_allowed_example {
            findings.push(ValidationFinding {
                severity: FindingSeverity::Warning,
                file: file.to_string(),
                message: format!("secret-like assignment near line {}", line_no + 1),
            });
        }
    }
}

fn scan_workflow_risks(file: &str, content: &str, findings: &mut Vec<ValidationFinding>) {
    for (line_no, line) in content.lines().enumerate() {
        let lower = line.trim().to_ascii_lowercase();
        if lower.starts_with("pull_request_target:") || lower == "pull_request_target" {
            findings.push(ValidationFinding {
                severity: FindingSeverity::Warning,
                file: file.to_string(),
                message: format!(
                    "pull_request_target requires explicit threat review near line {}",
                    line_no + 1
                ),
            });
        }
        if lower.contains("permissions:") && lower.contains("write-all") {
            findings.push(ValidationFinding {
                severity: FindingSeverity::Critical,
                file: file.to_string(),
                message: format!(
                    "workflow uses write-all permissions near line {}",
                    line_no + 1
                ),
            });
        }
        if lower.starts_with("if:") && lower.contains("secrets.") {
            findings.push(ValidationFinding {
                severity: FindingSeverity::Warning,
                file: file.to_string(),
                message: format!(
                    "workflow references secrets directly in if conditional near line {}",
                    line_no + 1
                ),
            });
        }
    }
}

fn is_github_workflow(file: &str) -> bool {
    file.starts_with(".github/workflows/") && (file.ends_with(".yml") || file.ends_with(".yaml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_secretish_lines() {
        let mut findings = Vec::new();
        let fake_secret = format!("let x = \"{}abcdef\";", ["gh", "p_"].concat());
        scan_secret_patterns("src/main.rs", &fake_secret, &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, FindingSeverity::Critical);
    }

    #[test]
    fn flags_committed_env_file() {
        let mut findings = Vec::new();
        flag_committed_env_file(".env.production", &mut findings);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, FindingSeverity::Critical);
    }
}
