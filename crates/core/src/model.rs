use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileCategory {
    Source,
    Config,
    Workflow,
    SecretCandidate,
    Documentation,
    Test,
    Build,
    Lockfile,
    AgentControl,
    Security,
    Unknown,
}

impl fmt::Display for FileCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            FileCategory::Source => "source",
            FileCategory::Config => "config",
            FileCategory::Workflow => "workflow",
            FileCategory::SecretCandidate => "secret-candidate",
            FileCategory::Documentation => "documentation",
            FileCategory::Test => "test",
            FileCategory::Build => "build",
            FileCategory::Lockfile => "lockfile",
            FileCategory::AgentControl => "agent-control",
            FileCategory::Security => "security",
            FileCategory::Unknown => "unknown",
        };
        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRecord {
    pub path: String,
    pub size_bytes: u64,
    pub extension: Option<String>,
    pub category: FileCategory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretReference {
    pub file: String,
    pub key: String,
    pub source: SecretSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSource {
    DotEnv,
    GitHubActionsSecret,
    GitHubActionsVariable,
    GitHubActionsEnv,
    ProcessEnv,
    ImportMetaEnv,
    PythonEnvVar,
    RustEnvVar,
    DenoEnv,
    Infisical,
    Sops,
    VaultOrOpenBao,
    Doppler,
    Direnv,
    DockerComposeEnvFile,
    MiseEnv,
    Unknown,
}

impl fmt::Display for SecretSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            SecretSource::DotEnv => ".env",
            SecretSource::GitHubActionsSecret => "github-actions-secret",
            SecretSource::GitHubActionsVariable => "github-actions-variable",
            SecretSource::GitHubActionsEnv => "github-actions-env",
            SecretSource::ProcessEnv => "process.env",
            SecretSource::ImportMetaEnv => "import.meta.env",
            SecretSource::PythonEnvVar => "python-env",
            SecretSource::RustEnvVar => "std::env::var",
            SecretSource::DenoEnv => "Deno.env.get",
            SecretSource::Infisical => "infisical",
            SecretSource::Sops => "sops",
            SecretSource::VaultOrOpenBao => "vault-or-openbao",
            SecretSource::Doppler => "doppler",
            SecretSource::Direnv => "direnv",
            SecretSource::DockerComposeEnvFile => "docker-compose-env-file",
            SecretSource::MiseEnv => "mise-env",
            SecretSource::Unknown => "unknown",
        };
        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInventory {
    pub name: String,
    pub root: String,
    pub files: Vec<FileRecord>,
    pub languages: BTreeMap<String, usize>,
    pub package_managers: Vec<String>,
    pub env_keys: Vec<String>,
    pub secret_refs: Vec<SecretReference>,
    pub entrypoints: Vec<String>,
    pub workflows: Vec<String>,
    pub agent_files: Vec<String>,
    pub security_files: Vec<String>,
}

impl RepoInventory {
    pub fn new(name: impl Into<String>, root: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            root: root.into(),
            files: Vec::new(),
            languages: BTreeMap::new(),
            package_managers: Vec::new(),
            env_keys: Vec::new(),
            secret_refs: Vec::new(),
            entrypoints: Vec::new(),
            workflows: Vec::new(),
            agent_files: Vec::new(),
            security_files: Vec::new(),
        }
    }

    pub fn count_by_category(&self) -> BTreeMap<FileCategory, usize> {
        let mut counts = BTreeMap::new();
        for file in &self.files {
            *counts.entry(file.category).or_insert(0) += 1;
        }
        counts
    }

    pub fn has_file(&self, path: &str) -> bool {
        self.files.iter().any(|f| f.path == path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeGap {
    pub id: &'static str,
    pub title: &'static str,
    pub risk: &'static str,
    pub applied_update: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationFinding {
    pub severity: FindingSeverity,
    pub file: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FindingSeverity {
    Info,
    Warning,
    Critical,
}

impl fmt::Display for FindingSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            FindingSeverity::Info => "info",
            FindingSeverity::Warning => "warning",
            FindingSeverity::Critical => "critical",
        };
        write!(f, "{value}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub size_bytes: u64,
    pub fnv1a64: String,
}
