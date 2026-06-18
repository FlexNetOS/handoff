use crate::fs_utils::{ensure_dir, write_string_preserving_existing};
use crate::manifest::write_manifest;
use crate::model::{MergeGap, RepoInventory};
use crate::scanner::scan_repo;
use crate::templates;
use std::collections::BTreeSet;
use std::path::Path;

pub fn known_gaps_and_updates() -> Vec<MergeGap> {
    vec![
        MergeGap {
            id: "GAP-001",
            title: "AI agents start editing before repository intent is mapped",
            risk: "High: causes duplicate abstractions, broken entrypoints, and unreviewable mega-PRs.",
            applied_update: "Generated inventories, feature matrix, API/env/secret contract docs, JSON sidecars, and task templates before merge work begins.",
        },
        MergeGap {
            id: "GAP-002",
            title: "Secrets and environment configuration are conflated",
            risk: "High: leaks secrets or creates CI/local drift.",
            applied_update: "Expanded extraction for dotenv, GitHub secrets/vars/env, Rust, Node, Vite, Python, Deno, SOPS, Infisical, Doppler, direnv, mise, Vault/OpenBao, and Compose env files.",
        },
        MergeGap {
            id: "GAP-003",
            title: "Parallel agent sessions create integration conflicts",
            risk: "High: multiple branches claim merge authority and overwrite each other.",
            applied_update: "Added `.idd/LOCK.md`, `AI_MERGE/08_agent_queue.md`, and AGENTS.md rule: many agents may analyze, one integration branch has authority.",
        },
        MergeGap {
            id: "GAP-004",
            title: "No reproducible local/CI toolchain contract",
            risk: "Medium: repo works for one agent but fails for another.",
            applied_update: "Kept dependency-free Rust implementation, added GitHub CI, PR/issue templates, manifesting, and validation gates.",
        },
        MergeGap {
            id: "GAP-005",
            title: "Feature merge lacks rollback and parity evidence",
            risk: "Medium: old features disappear during cleanup.",
            applied_update: "Added parity test plan, PR evidence requirements, migration notes, deprecate-before-delete rule, and task Definition of Done checklist.",
        },
        MergeGap {
            id: "GAP-006",
            title: "Repository instructions are not optimized for current GitHub agents",
            risk: "Medium: agents waste context, miss rules, or produce broad changes.",
            applied_update: "Added `AGENTS.md`, `.github/copilot-instructions.md`, issue template, and PR template so agent prompts are repo-native and reviewable.",
        },
        MergeGap {
            id: "GAP-007",
            title: "Cloud agents cannot safely mutate two repos in one run",
            risk: "High: multi-repo tasks exceed agent limits or silently drop changes.",
            applied_update: "Added GitHub execution plan requiring imports/mirrors into one integration repo and one task branch per issue.",
        },
        MergeGap {
            id: "GAP-008",
            title: "Generated artifacts are overwritten without an audit trail",
            risk: "Medium: agent reruns erase prior decisions.",
            applied_update: "Added backup-on-overwrite writes and `.idd/MANIFEST.tsv` generation using deterministic file hashes.",
        },
    ]
}

pub fn generate_plan_from_paths(
    repo_a: impl AsRef<Path>,
    repo_b: impl AsRef<Path>,
    out: impl AsRef<Path>,
    name: &str,
) -> Result<(), String> {
    let a = scan_repo(repo_a)?;
    let b = scan_repo(repo_b)?;
    generate_workspace(&a, &b, out, name)
}

pub fn generate_workspace(
    a: &RepoInventory,
    b: &RepoInventory,
    out: impl AsRef<Path>,
    name: &str,
) -> Result<(), String> {
    let out = out.as_ref();
    let ai_merge = out.join("AI_MERGE");
    let tasks = ai_merge.join("07_tasks");
    ensure_dir(&tasks).map_err(|e| format!("failed to create output directories: {e}"))?;
    ensure_dir(&out.join(".idd")).map_err(|e| format!("failed to create .idd: {e}"))?;
    ensure_dir(&out.join(".github/workflows"))
        .map_err(|e| format!("failed to create workflows: {e}"))?;
    ensure_dir(&out.join(".github/ISSUE_TEMPLATE"))
        .map_err(|e| format!("failed to create issue templates: {e}"))?;

    write_preserve(&out.join("AGENTS.md"), templates::AGENTS_MD)?;
    write_preserve(
        &out.join(".github/copilot-instructions.md"),
        templates::COPILOT_INSTRUCTIONS,
    )?;
    write_preserve(
        &out.join(".github/pull_request_template.md"),
        templates::PR_TEMPLATE,
    )?;
    write_preserve(
        &out.join(".github/ISSUE_TEMPLATE/idd-task.yml"),
        templates::ISSUE_TEMPLATE,
    )?;
    write_preserve(&out.join("SECURITY.md"), templates::SECURITY_MD)?;
    write_preserve(&out.join(".idd/LOCK.md"), templates::LOCK_TEMPLATE)?;
    write_preserve(
        &out.join(".env.schema.example.json"),
        templates::ENV_SCHEMA_EXAMPLE,
    )?;
    write_preserve(
        &out.join(".github/workflows/idd-ci.yml"),
        templates::GITHUB_ACTIONS_CI,
    )?;

    write_preserve(
        &ai_merge.join("00_repo_a_inventory.md"),
        &inventory_markdown(a),
    )?;
    write_preserve(
        &ai_merge.join("00_repo_a_inventory.json"),
        &inventory_json(a),
    )?;
    write_preserve(
        &ai_merge.join("01_repo_b_inventory.md"),
        &inventory_markdown(b),
    )?;
    write_preserve(
        &ai_merge.join("01_repo_b_inventory.json"),
        &inventory_json(b),
    )?;
    write_preserve(
        &ai_merge.join("02_feature_matrix.md"),
        &feature_matrix_markdown(a, b),
    )?;
    write_preserve(
        &ai_merge.join("03_env_and_secret_contracts.md"),
        &env_secret_contract_markdown(a, b),
    )?;
    write_preserve(
        &ai_merge.join("03_env_and_secret_contracts.json"),
        &env_secret_contract_json(a, b),
    )?;
    write_preserve(
        &ai_merge.join("04_merge_plan.md"),
        &merge_plan_markdown(name, a, b),
    )?;
    write_preserve(
        &ai_merge.join("05_conflict_risk_register.md"),
        &conflict_register_markdown(a, b),
    )?;
    write_preserve(
        &ai_merge.join("06_gap_audit_and_applied_updates.md"),
        &gap_audit_markdown(),
    )?;
    write_preserve(&ai_merge.join("08_agent_queue.md"), templates::AGENT_QUEUE)?;
    write_preserve(
        &ai_merge.join("09_github_execution.md"),
        templates::GITHUB_EXECUTION,
    )?;
    write_preserve(
        &ai_merge.join("10_parity_test_plan.md"),
        templates::PARITY_TEST_PLAN,
    )?;
    write_preserve(
        &ai_merge.join("11_provider_matrix.md"),
        templates::PROVIDER_MATRIX,
    )?;

    write_preserve(
        &tasks.join("0001-import-repos-without-flattening.md"),
        &task_markdown(
            "Import both repositories under /imports without flattening",
            "repo-import",
        ),
    )?;
    write_preserve(
        &tasks.join("0002-normalize-env-and-secrets.md"),
        &task_markdown("Normalize environment and secrets contracts", "env-secrets"),
    )?;
    write_preserve(
        &tasks.join("0003-create-canonical-interfaces.md"),
        &task_markdown(
            "Create canonical interfaces before moving implementations",
            "interface",
        ),
    )?;
    write_preserve(
        &tasks.join("0004-add-parity-tests.md"),
        &task_markdown("Add parity tests for migrated behavior", "testing"),
    )?;
    write_preserve(
        &tasks.join("0005-wire-github-ci-and-validation.md"),
        &task_markdown(
            "Wire GitHub CI, validation, PR template, and issue template",
            "github",
        ),
    )?;

    write_manifest(out, out.join(".idd/MANIFEST.tsv"))?;
    Ok(())
}

pub fn inventory_markdown(inv: &RepoInventory) -> String {
    let mut out = String::new();
    out.push_str(&format!("# Repository Inventory: {}\n\n", inv.name));
    out.push_str(&format!("- Root: `{}`\n", inv.root));
    out.push_str(&format!("- Files scanned: `{}`\n", inv.files.len()));
    out.push_str("\n## Category Counts\n\n| Category | Count |\n|---|---:|\n");
    for (category, count) in inv.count_by_category() {
        out.push_str(&format!("| {category} | {count} |\n"));
    }
    out.push_str("\n## Languages\n\n| Language | Files |\n|---|---:|\n");
    for (language, count) in &inv.languages {
        out.push_str(&format!("| {language} | {count} |\n"));
    }
    out.push_str("\n## Package Managers / Toolchains\n\n");
    push_list(&mut out, &inv.package_managers);
    out.push_str("\n## Entrypoints\n\n");
    push_list(&mut out, &inv.entrypoints);
    out.push_str("\n## Workflows\n\n");
    push_list(&mut out, &inv.workflows);
    out.push_str("\n## Agent Control Files\n\n");
    push_list(&mut out, &inv.agent_files);
    out.push_str("\n## Security Files\n\n");
    push_list(&mut out, &inv.security_files);
    out.push_str("\n## Environment Keys Found\n\n");
    push_list(&mut out, &inv.env_keys);
    out.push_str("\n## Secret / Env References Found\n\n| File | Key | Source |\n|---|---|---|\n");
    if inv.secret_refs.is_empty() {
        out.push_str("| _none detected_ |  |  |\n");
    } else {
        for r in &inv.secret_refs {
            out.push_str(&format!("| `{}` | `{}` | {} |\n", r.file, r.key, r.source));
        }
    }
    out.push_str("\n## File Index\n\n| Path | Category | Size |\n|---|---|---:|\n");
    for file in &inv.files {
        out.push_str(&format!(
            "| `{}` | {} | {} |\n",
            file.path, file.category, file.size_bytes
        ));
    }
    out
}

pub fn inventory_json(inv: &RepoInventory) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"name\": \"{}\",\n", json_escape(&inv.name)));
    out.push_str(&format!("  \"root\": \"{}\",\n", json_escape(&inv.root)));
    out.push_str(&format!("  \"files_scanned\": {},\n", inv.files.len()));
    out.push_str("  \"languages\": {");
    for (idx, (language, count)) in inv.languages.iter().enumerate() {
        if idx > 0 {
            out.push_str(", ");
        }
        out.push_str(&format!("\"{}\": {}", json_escape(language), count));
    }
    out.push_str("},\n");
    out.push_str(&format!(
        "  \"package_managers\": {},\n",
        json_array(&inv.package_managers)
    ));
    out.push_str(&format!(
        "  \"entrypoints\": {},\n",
        json_array(&inv.entrypoints)
    ));
    out.push_str(&format!(
        "  \"workflows\": {},\n",
        json_array(&inv.workflows)
    ));
    out.push_str(&format!(
        "  \"agent_files\": {},\n",
        json_array(&inv.agent_files)
    ));
    out.push_str(&format!(
        "  \"security_files\": {},\n",
        json_array(&inv.security_files)
    ));
    out.push_str(&format!("  \"env_keys\": {},\n", json_array(&inv.env_keys)));
    out.push_str("  \"secret_refs\": [\n");
    for (idx, r) in inv.secret_refs.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!(
            "    {{\"file\": \"{}\", \"key\": \"{}\", \"source\": \"{}\"}}",
            json_escape(&r.file),
            json_escape(&r.key),
            json_escape(&r.source.to_string())
        ));
    }
    out.push_str("\n  ]\n}\n");
    out
}

pub fn feature_matrix_markdown(a: &RepoInventory, b: &RepoInventory) -> String {
    let mut out = String::from("# Feature Matrix\n\n");
    out.push_str("This matrix is generated from structural signals. Treat it as a starting point, then refine it with explicit product intent.\n\n");
    out.push_str("| Capability | Repo A Signal | Repo B Signal | Default Decision | Migration Action |\n|---|---|---|---|---|\n");

    let capabilities = [
        (
            "Rust native core",
            has_language(a, "Rust"),
            has_language(b, "Rust"),
        ),
        (
            "Node/TypeScript UI or tooling",
            has_language(a, "TypeScript") || has_language(a, "JavaScript"),
            has_language(b, "TypeScript") || has_language(b, "JavaScript"),
        ),
        (
            "Python tooling",
            has_language(a, "Python"),
            has_language(b, "Python"),
        ),
        (
            "GitHub Actions CI",
            !a.workflows.is_empty(),
            !b.workflows.is_empty(),
        ),
        (
            "Environment contract",
            !a.env_keys.is_empty(),
            !b.env_keys.is_empty(),
        ),
        (
            "Secret references",
            !a.secret_refs.is_empty(),
            !b.secret_refs.is_empty(),
        ),
        (
            "Nix, mise, or direnv toolchain",
            has_manager(a, "nix") || has_manager(a, "mise") || has_manager(a, "direnv"),
            has_manager(b, "nix") || has_manager(b, "mise") || has_manager(b, "direnv"),
        ),
        (
            "Agent control files",
            !a.agent_files.is_empty(),
            !b.agent_files.is_empty(),
        ),
        (
            "Security policy files",
            !a.security_files.is_empty(),
            !b.security_files.is_empty(),
        ),
    ];

    for (cap, av, bv) in capabilities {
        let decision = match (av, bv) {
            (true, true) => "Compare and select canonical implementation",
            (true, false) => "Keep Repo A implementation unless tests fail",
            (false, true) => "Migrate Repo B implementation into canonical module",
            (false, false) => "Create only if required by product intent",
        };
        let action = match (av, bv) {
            (true, true) => "Write parity tests, then deduplicate",
            (true, false) | (false, true) => "Wrap behind stable interface",
            (false, false) => "No action yet",
        };
        out.push_str(&format!(
            "| {cap} | {} | {} | {decision} | {action} |\n",
            yes_no(av),
            yes_no(bv)
        ));
    }

    out.push_str("\n## Shared Paths\n\n| Path | Repo A | Repo B | Risk |\n|---|---|---|---|\n");
    for path in shared_paths(a, b).into_iter().take(100) {
        out.push_str(&format!(
            "| `{path}` | yes | yes | naming/API collision |\n"
        ));
    }
    out
}

pub fn env_secret_contract_markdown(a: &RepoInventory, b: &RepoInventory) -> String {
    let keys = unified_env_keys(a, b);
    let mut out = String::from("# Environment and Secrets Contract\n\n");
    out.push_str("Rule: configuration keys may be documented in Git; secret values must not be committed. Prefer OIDC over long-lived cloud credentials when possible.\n\n");
    out.push_str(
        "| Key | Repo A | Repo B | Secret? | Canonical Decision |\n|---|---|---|---|---|\n",
    );
    if keys.is_empty() {
        out.push_str("| _none detected_ |  |  |  | Add keys intentionally as needed |\n");
    } else {
        for key in keys {
            let in_a = contains_key(a, &key);
            let in_b = contains_key(b, &key);
            let secret = looks_secretish(&key);
            out.push_str(&format!(
                "| `{}` | {} | {} | {} | TBD: keep/rename/deprecate/provider-map |\n",
                key,
                yes_no(in_a),
                yes_no(in_b),
                yes_no(secret)
            ));
        }
    }

    out.push_str("\n## Canonical Resolution Order\n\n");
    out.push_str("1. Explicit CLI flag\n2. Process environment\n3. Secret provider adapter\n4. `.env` for local development only\n5. Safe default from checked-in config\n\n");
    out.push_str("## Required Adapters\n\n- `ConfigProvider`: loads non-secret config.\n- `SecretProvider`: resolves secret values without exposing them in logs.\n- `EnvResolver`: merges explicit, environment, provider, and default values.\n\n");
    out.push_str("## Provider decision required before implementation\n\nFor each secret namespace, decide whether the source of truth is GitHub Actions secrets, OIDC, SOPS, Infisical, OpenBao/Vault, Doppler, direnv/mise local env, or another explicitly documented backend.\n");
    out
}

pub fn env_secret_contract_json(a: &RepoInventory, b: &RepoInventory) -> String {
    let keys = unified_env_keys(a, b);
    let mut out = String::from("{\n  \"keys\": [\n");
    for (idx, key) in keys.iter().enumerate() {
        if idx > 0 {
            out.push_str(",\n");
        }
        out.push_str(&format!(
            "    {{\"key\": \"{}\", \"repo_a\": {}, \"repo_b\": {}, \"secretish\": {}}}",
            json_escape(key),
            contains_key(a, key),
            contains_key(b, key),
            looks_secretish(key)
        ));
    }
    out.push_str("\n  ],\n  \"resolution_order\": [\"cli_flag\", \"process_env\", \"secret_provider\", \"local_dotenv\", \"checked_in_default\"]\n}\n");
    out
}

pub fn merge_plan_markdown(name: &str, a: &RepoInventory, b: &RepoInventory) -> String {
    format!(
        "# Merge Plan: {name}\n\n\
## Strategy\n\n\
Use an intent-driven integration branch. Keep both imported repos intact until canonical contracts are proven. Migrate one vertical slice per PR.\n\n\
## Recommended Tree\n\n```text\n\
imports/{a_name}/          # untouched Repo A import\n\
imports/{b_name}/          # untouched Repo B import\n\
crates/                    # canonical Rust crates\n\
apps/                      # canonical runnable apps\n\
docs/                      # user and operator docs\n\
AI_MERGE/                  # agent-readable control plane\n\
.idd/                      # lock, manifest, local state\n\
```\n\n\
## Execution Phases\n\n\
1. Freeze integration branch and write `.idd/LOCK.md`.\n\
2. Import both repositories under `/imports`.\n\
3. Normalize environment and secret contracts.\n\
4. Create canonical interfaces before moving implementations.\n\
5. Migrate the smallest working vertical slice.\n\
6. Add parity tests comparing old behavior to new behavior.\n\
7. Deprecate old paths only after tests pass.\n\
8. Remove duplicate code in final cleanup PRs.\n\n\
## Initial Risk Read\n\n\
- Repo A files: `{a_files}`\n\
- Repo B files: `{b_files}`\n\
- Shared path collisions: `{collisions}`\n\
- Repo A secret/env references: `{a_secrets}`\n\
- Repo B secret/env references: `{b_secrets}`\n\n\
## Merge Gate\n\n\
A PR is mergeable only when build, tests, lint/typecheck, secret scan, `idd validate`, and migration notes are complete.\n\n\
## GitHub Agent Constraint\n\n\
Cloud agents should be fed one repo task at a time. If the task needs two repos, import or mirror the second repo into this integration repo first, then assign a single narrow PR.\n",
        name = name,
        a_name = a.name,
        b_name = b.name,
        a_files = a.files.len(),
        b_files = b.files.len(),
        collisions = shared_paths(a, b).len(),
        a_secrets = a.secret_refs.len(),
        b_secrets = b.secret_refs.len(),
    )
}

pub fn conflict_register_markdown(a: &RepoInventory, b: &RepoInventory) -> String {
    let mut out = String::from(
        "# Conflict Risk Register\n\n| Risk | Evidence | Mitigation |\n|---|---|---|\n",
    );
    let shared = shared_paths(a, b);
    if shared.is_empty() {
        out.push_str("| Path collisions | None detected by identical relative path | Keep monitoring as files move |\n");
    } else {
        out.push_str(&format!(
            "| Path collisions | {} identical relative paths | Keep imports isolated; migrate into canonical modules one PR at a time |\n",
            shared.len()
        ));
    }
    if !a.secret_refs.is_empty() || !b.secret_refs.is_empty() {
        out.push_str("| Secret/config drift | Secret/env references found | Define one SecretProvider interface and one env resolution order |\n");
    }
    if has_multiple_package_managers(a) || has_multiple_package_managers(b) {
        out.push_str("| Toolchain drift | Multiple package managers detected | Define canonical toolchain and pin versions |\n");
    }
    if !a.workflows.is_empty() || !b.workflows.is_empty() {
        out.push_str("| CI drift | Workflow files detected | Merge CI by job intent, not by copying both workflow files blindly |\n");
    }
    out.push_str("| Agent conflict | Parallel agent branches possible | Use `.idd/LOCK.md` and `AI_MERGE/08_agent_queue.md`; only integration branch has merge authority |\n");
    out
}

pub fn gap_audit_markdown() -> String {
    let mut out = String::from("# Gap Audit and Applied Updates\n\n| ID | Gap | Risk | Applied Update |\n|---|---|---|---|\n");
    for gap in known_gaps_and_updates() {
        out.push_str(&format!(
            "| {} | {} | {} | {} |\n",
            gap.id, gap.title, gap.risk, gap.applied_update
        ));
    }
    out
}

pub fn task_markdown(title: &str, kind: &str) -> String {
    templates::TASK_TEMPLATE
        .replace("{{TITLE}}", title)
        .replace("{{KIND}}", kind)
}

pub fn write_task(out: impl AsRef<Path>, title: &str, kind: &str) -> Result<(), String> {
    let slug = slugify(title);
    let path = out.as_ref().join(format!("{slug}.md"));
    write_preserve(&path, &task_markdown(title, kind))
}

fn write_preserve(path: &Path, content: &str) -> Result<(), String> {
    write_string_preserving_existing(path, content)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

fn push_list(out: &mut String, values: &[String]) {
    if values.is_empty() {
        out.push_str("- _none detected_\n");
    } else {
        for value in values {
            out.push_str(&format!("- `{}`\n", value));
        }
    }
}

fn has_language(inv: &RepoInventory, language: &str) -> bool {
    inv.languages.contains_key(language)
}

fn has_manager(inv: &RepoInventory, manager: &str) -> bool {
    inv.package_managers.iter().any(|m| m == manager)
}

fn has_multiple_package_managers(inv: &RepoInventory) -> bool {
    inv.package_managers.len() > 1
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn shared_paths(a: &RepoInventory, b: &RepoInventory) -> Vec<String> {
    let a_paths = a
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect::<BTreeSet<_>>();
    let b_paths = b
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect::<BTreeSet<_>>();
    a_paths.intersection(&b_paths).cloned().collect()
}

fn unified_env_keys(a: &RepoInventory, b: &RepoInventory) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for key in &a.env_keys {
        keys.insert(key.clone());
    }
    for key in &b.env_keys {
        keys.insert(key.clone());
    }
    for r in &a.secret_refs {
        keys.insert(r.key.clone());
    }
    for r in &b.secret_refs {
        keys.insert(r.key.clone());
    }
    keys.into_iter().collect()
}

fn contains_key(inv: &RepoInventory, key: &str) -> bool {
    inv.env_keys.iter().any(|k| k == key) || inv.secret_refs.iter().any(|r| r.key == key)
}

fn looks_secretish(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    [
        "SECRET",
        "TOKEN",
        "KEY",
        "PASSWORD",
        "PASS",
        "PRIVATE",
        "CREDENTIAL",
        "AUTH",
    ]
    .iter()
    .any(|needle| upper.contains(*needle))
}

fn slugify(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

fn json_array(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|v| format!("\"{}\"", json_escape(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{items}]")
}

fn json_escape(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{FileCategory, FileRecord, SecretReference, SecretSource};
    use tempfile::tempdir;

    fn mock_inventory(name: &str) -> RepoInventory {
        RepoInventory {
            name: name.to_string(),
            root: ".".to_string(),
            files: vec![FileRecord {
                path: "src/main.rs".to_string(),
                size_bytes: 100,
                extension: Some("rs".to_string()),
                category: FileCategory::Source,
            }],
            languages: [("Rust".to_string(), 1)].into_iter().collect(),
            package_managers: vec!["cargo".to_string()],
            env_keys: vec!["PORT".to_string()],
            secret_refs: vec![SecretReference {
                file: "src/main.rs".to_string(),
                key: "API_KEY".to_string(),
                source: SecretSource::ProcessEnv,
            }],
            entrypoints: vec!["src/main.rs".to_string()],
            workflows: vec![],
            agent_files: vec![],
            security_files: vec![],
        }
    }

    #[test]
    fn test_generate_workspace_creates_files() {
        let tmp = tempdir().unwrap();
        let a = mock_inventory("repo-a");
        let b = mock_inventory("repo-b");
        generate_workspace(&a, &b, tmp.path(), "test-unification").unwrap();

        assert!(tmp.path().join("AI_MERGE/02_feature_matrix.md").exists());
        assert!(tmp.path().join("AI_MERGE/04_merge_plan.md").exists());
        assert!(tmp.path().join(".idd/MANIFEST.tsv").exists());
    }

    #[test]
    fn test_inventory_json_validity() {
        let inv = mock_inventory("test-repo");
        let json = inventory_json(&inv);
        assert!(json.contains("\"name\": \"test-repo\""));
        assert!(json.contains("\"PORT\""));
    }

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("My Task Name"), "my-task-name");
        assert_eq!(
            slugify("Task/With_Special!Chars"),
            "task-with-special-chars"
        );
    }

    #[test]
    fn test_looks_secretish() {
        assert!(looks_secretish("DATABASE_URL_PASSWORD"));
        assert!(looks_secretish("AWS_ACCESS_KEY"));
        assert!(!looks_secretish("APP_PORT"));
    }
}
