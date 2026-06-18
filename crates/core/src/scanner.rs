use crate::env_contract::{
    extract_env_keys_from_dotenv, extract_secret_refs, implied_tool_reference,
};
use crate::fs_utils::{read_to_string_lossy, relative_path, stable_walk};
use crate::model::{FileCategory, FileRecord, RepoInventory, SecretSource};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const MAX_TEXT_SCAN_BYTES: u64 = 512 * 1024;

pub fn scan_repo(path: impl AsRef<Path>) -> Result<RepoInventory, String> {
    let root = path.as_ref();
    if !root.exists() {
        return Err(format!("repo path does not exist: {}", root.display()));
    }
    if !root.is_dir() {
        return Err(format!("repo path is not a directory: {}", root.display()));
    }

    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("repo")
        .to_string();

    let mut inventory = RepoInventory::new(name, root.display().to_string());
    let files = stable_walk(root).map_err(|e| format!("walk failed: {e}"))?;
    let mut env_keys = BTreeSet::new();
    let mut package_managers = BTreeSet::new();
    let mut entrypoints = BTreeSet::new();
    let mut workflows = BTreeSet::new();
    let mut agent_files = BTreeSet::new();
    let mut security_files = BTreeSet::new();

    for abs_path in files {
        let rel = relative_path(root, &abs_path);
        let metadata = match fs::metadata(&abs_path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let ext = abs_path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        let category = classify_file(&rel, ext.as_deref());

        if let Some(lang) = language_from_extension(ext.as_deref()) {
            *inventory.languages.entry(lang.to_string()).or_insert(0) += 1;
        }

        detect_package_manager(&rel, &mut package_managers);
        detect_entrypoint(&rel, &mut entrypoints);
        if is_workflow(&rel) {
            workflows.insert(rel.clone());
        }
        if matches!(category, FileCategory::AgentControl) {
            agent_files.insert(rel.clone());
        }
        if matches!(category, FileCategory::Security) {
            security_files.insert(rel.clone());
        }

        let content = read_to_string_lossy(&abs_path, MAX_TEXT_SCAN_BYTES).unwrap_or_default();
        if is_dotenv_like(&abs_path, &rel) {
            for key in extract_env_keys_from_dotenv(&content) {
                env_keys.insert(key.clone());
                inventory.secret_refs.push(crate::model::SecretReference {
                    file: rel.clone(),
                    key,
                    source: SecretSource::DotEnv,
                });
            }
        }
        for r in extract_secret_refs(&rel, &content) {
            env_keys.insert(r.key.clone());
            inventory.secret_refs.push(r);
        }
        if let Some(r) = implied_tool_reference(&rel) {
            inventory.secret_refs.push(r);
        }

        inventory.files.push(FileRecord {
            path: rel,
            size_bytes: metadata.len(),
            extension: ext,
            category,
        });
    }

    inventory.package_managers = package_managers.into_iter().collect();
    inventory.entrypoints = entrypoints.into_iter().collect();
    inventory.workflows = workflows.into_iter().collect();
    inventory.agent_files = agent_files.into_iter().collect();
    inventory.security_files = security_files.into_iter().collect();
    inventory.env_keys = env_keys.into_iter().collect();
    inventory.files.sort_by(|a, b| a.path.cmp(&b.path));
    inventory
        .secret_refs
        .sort_by(|a, b| a.file.cmp(&b.file).then(a.key.cmp(&b.key)));
    inventory
        .secret_refs
        .dedup_by(|a, b| a.file == b.file && a.key == b.key && a.source == b.source);
    Ok(inventory)
}

pub fn classify_file(path: &str, ext: Option<&str>) -> FileCategory {
    let lower = path.to_ascii_lowercase();
    let file_name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_ascii_lowercase();

    if lower.starts_with("ai_merge/")
        || lower.starts_with(".idd/")
        || file_name == "agents.md"
        || lower == ".github/copilot-instructions.md"
    {
        return FileCategory::AgentControl;
    }
    if lower.starts_with(".github/workflows/") || lower.contains("/.github/workflows/") {
        return FileCategory::Workflow;
    }
    if matches!(
        file_name.as_str(),
        "security.md" | "codeowners" | "dependabot.yml" | "dependabot.yaml"
    ) {
        return FileCategory::Security;
    }
    if file_name.starts_with(".env") && file_name != ".env.example" {
        return FileCategory::SecretCandidate;
    }
    if matches!(
        file_name.as_str(),
        "cargo.lock"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "bun.lockb"
            | "bun.lock"
            | "uv.lock"
            | "poetry.lock"
            | "go.sum"
    ) {
        return FileCategory::Lockfile;
    }
    if matches!(
        file_name.as_str(),
        "cargo.toml"
            | "package.json"
            | "pyproject.toml"
            | "go.mod"
            | "flake.nix"
            | "mise.toml"
            | "justfile"
            | "makefile"
            | "dockerfile"
    ) {
        return FileCategory::Build;
    }
    if lower.contains("test") || lower.contains("spec") || lower.starts_with("tests/") {
        return FileCategory::Test;
    }
    if matches!(ext, Some("md") | Some("mdx") | Some("rst") | Some("txt")) {
        return FileCategory::Documentation;
    }
    if matches!(
        ext,
        Some("toml")
            | Some("yaml")
            | Some("yml")
            | Some("json")
            | Some("jsonc")
            | Some("ini")
            | Some("conf")
            | Some("hcl")
    ) {
        return FileCategory::Config;
    }
    if language_from_extension(ext).is_some() {
        return FileCategory::Source;
    }
    FileCategory::Unknown
}

fn language_from_extension(ext: Option<&str>) -> Option<&'static str> {
    match ext {
        Some("rs") => Some("Rust"),
        Some("ts") | Some("tsx") => Some("TypeScript"),
        Some("js") | Some("jsx") | Some("mjs") | Some("cjs") => Some("JavaScript"),
        Some("py") => Some("Python"),
        Some("go") => Some("Go"),
        Some("java") => Some("Java"),
        Some("kt") | Some("kts") => Some("Kotlin"),
        Some("swift") => Some("Swift"),
        Some("c") | Some("h") => Some("C"),
        Some("cc") | Some("cpp") | Some("hpp") | Some("cxx") => Some("C++"),
        Some("cs") => Some("C#"),
        Some("rb") => Some("Ruby"),
        Some("php") => Some("PHP"),
        Some("sh") | Some("bash") | Some("zsh") | Some("nu") => Some("Shell"),
        Some("nix") => Some("Nix"),
        Some("sql") => Some("SQL"),
        Some("hcl") => Some("HCL"),
        _ => None,
    }
}

fn detect_package_manager(path: &str, managers: &mut BTreeSet<String>) {
    let lower = path.to_ascii_lowercase();
    match lower.as_str() {
        "cargo.toml" => {
            managers.insert("cargo".to_string());
        }
        "package.json" => {
            managers.insert("node".to_string());
        }
        "pnpm-lock.yaml" => {
            managers.insert("pnpm".to_string());
        }
        "yarn.lock" => {
            managers.insert("yarn".to_string());
        }
        "bun.lockb" | "bun.lock" => {
            managers.insert("bun".to_string());
        }
        "pyproject.toml" => {
            managers.insert("python/pyproject".to_string());
        }
        "uv.lock" => {
            managers.insert("uv".to_string());
        }
        "poetry.lock" => {
            managers.insert("poetry".to_string());
        }
        "go.mod" => {
            managers.insert("go".to_string());
        }
        "flake.nix" => {
            managers.insert("nix".to_string());
        }
        "mise.toml" => {
            managers.insert("mise".to_string());
        }
        ".envrc" => {
            managers.insert("direnv".to_string());
        }
        _ => {}
    }
}

fn detect_entrypoint(path: &str, entrypoints: &mut BTreeSet<String>) {
    let candidates = [
        "src/main.rs",
        "main.rs",
        "src/index.ts",
        "src/index.tsx",
        "src/main.ts",
        "src/main.tsx",
        "index.js",
        "main.py",
        "app.py",
        "cmd/main.go",
        "main.go",
    ];
    if candidates.contains(&path) {
        entrypoints.insert(path.to_string());
    }
}

fn is_workflow(path: &str) -> bool {
    path.starts_with(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

fn is_dotenv_like(abs_path: &Path, rel: &str) -> bool {
    let name = abs_path.file_name().and_then(|s| s.to_str()).unwrap_or(rel);
    name.starts_with(".env") || name == "env.example" || name == ".env.example"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_common_files() {
        assert_eq!(
            classify_file("src/main.rs", Some("rs")),
            FileCategory::Source
        );
        assert_eq!(
            classify_file("Cargo.toml", Some("toml")),
            FileCategory::Build
        );
        assert_eq!(
            classify_file(".github/workflows/ci.yml", Some("yml")),
            FileCategory::Workflow
        );
        assert_eq!(classify_file(".env", None), FileCategory::SecretCandidate);
        assert_eq!(
            classify_file("AGENTS.md", Some("md")),
            FileCategory::AgentControl
        );
    }
}
