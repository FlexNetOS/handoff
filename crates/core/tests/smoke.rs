use rusty_idd_core::planner::generate_workspace;
use rusty_idd_core::scanner::scan_repo;
use rusty_idd_core::validation::validate_workspace;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("idd-{name}-{nanos}"));
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn scans_repo_and_generates_workspace() {
    let repo_a = temp_dir("repo-a");
    fs::create_dir_all(repo_a.join("src")).unwrap();
    fs::write(
        repo_a.join("Cargo.toml"),
        "[package]\nname=\"a\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
    )
    .unwrap();
    fs::write(
        repo_a.join("src/main.rs"),
        "fn main() { let _ = std::env::var(\"DATABASE_URL\"); }\n",
    )
    .unwrap();
    fs::write(
        repo_a.join(".env.example"),
        "DATABASE_URL=postgres://localhost\n",
    )
    .unwrap();

    let repo_b = temp_dir("repo-b");
    fs::create_dir_all(repo_b.join(".github/workflows")).unwrap();
    fs::write(
        repo_b.join("package.json"),
        "{\"scripts\":{\"test\":\"echo ok\"}}\n",
    )
    .unwrap();
    fs::write(
        repo_b.join(".github/workflows/ci.yml"),
        "env:\n  API_KEY: ${{ secrets.API_KEY }}\n",
    )
    .unwrap();
    fs::write(
        repo_b.join("index.js"),
        "console.log(process.env['NODE_ENV']);\n",
    )
    .unwrap();

    let inv_a = scan_repo(&repo_a).unwrap();
    let inv_b = scan_repo(&repo_b).unwrap();
    assert!(inv_a.languages.contains_key("Rust"));
    assert!(inv_b.package_managers.contains(&"node".to_string()));
    assert!(inv_b.env_keys.contains(&"NODE_ENV".to_string()));

    let out = temp_dir("workspace");
    generate_workspace(&inv_a, &inv_b, &out, "smoke").unwrap();
    assert!(out.join("AI_MERGE/04_merge_plan.md").exists());
    assert!(out
        .join("AI_MERGE/03_env_and_secret_contracts.json")
        .exists());
    assert!(out.join("AI_MERGE/08_agent_queue.md").exists());
    assert!(out.join(".github/ISSUE_TEMPLATE/idd-task.yml").exists());
    assert!(out.join(".idd/MANIFEST.tsv").exists());

    let findings = validate_workspace(&out).unwrap();
    assert!(findings
        .iter()
        .all(|f| f.severity.to_string() != "critical"));
}
