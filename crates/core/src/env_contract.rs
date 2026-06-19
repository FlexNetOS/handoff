use crate::model::{SecretReference, SecretSource};
use std::collections::BTreeSet;
use std::path::Path;

pub fn extract_env_keys_from_dotenv(content: &str) -> Vec<String> {
    let mut keys = BTreeSet::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with("export ") && !line.contains('=')
        {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        if let Some((key, _value)) = line.split_once('=') {
            let key = key.trim();
            if is_env_key(key) {
                keys.insert(key.to_string());
            }
        }
    }
    keys.into_iter().collect()
}

pub fn extract_secret_refs(file: &str, content: &str) -> Vec<SecretReference> {
    let mut refs = Vec::new();

    refs.extend(extract_after_marker(
        file,
        content,
        "${{ secrets.",
        SecretSource::GitHubActionsSecret,
    ));
    refs.extend(extract_bracket_marker(
        file,
        content,
        "${{ secrets[",
        SecretSource::GitHubActionsSecret,
    ));
    refs.extend(extract_after_marker(
        file,
        content,
        "${{ vars.",
        SecretSource::GitHubActionsVariable,
    ));
    refs.extend(extract_bracket_marker(
        file,
        content,
        "${{ vars[",
        SecretSource::GitHubActionsVariable,
    ));
    refs.extend(extract_after_marker(
        file,
        content,
        "${{ env.",
        SecretSource::GitHubActionsEnv,
    ));
    refs.extend(extract_bracket_marker(
        file,
        content,
        "${{ env[",
        SecretSource::GitHubActionsEnv,
    ));

    refs.extend(extract_after_marker(
        file,
        content,
        "process.env.",
        SecretSource::ProcessEnv,
    ));
    refs.extend(extract_bracket_marker(
        file,
        content,
        "process.env[",
        SecretSource::ProcessEnv,
    ));
    refs.extend(extract_after_marker(
        file,
        content,
        "import.meta.env.",
        SecretSource::ImportMetaEnv,
    ));
    refs.extend(extract_bracket_marker(
        file,
        content,
        "import.meta.env[",
        SecretSource::ImportMetaEnv,
    ));

    refs.extend(extract_quoted_arg(
        file,
        content,
        "std::env::var(",
        SecretSource::RustEnvVar,
    ));
    refs.extend(extract_quoted_arg(
        file,
        content,
        "std::env::var_os(",
        SecretSource::RustEnvVar,
    ));
    refs.extend(extract_quoted_arg(
        file,
        content,
        "env::var(",
        SecretSource::RustEnvVar,
    ));
    refs.extend(extract_quoted_arg(
        file,
        content,
        "env::var_os(",
        SecretSource::RustEnvVar,
    ));
    refs.extend(extract_quoted_arg(
        file,
        content,
        "Deno.env.get(",
        SecretSource::DenoEnv,
    ));

    refs.extend(extract_bracket_marker(
        file,
        content,
        "os.environ[",
        SecretSource::PythonEnvVar,
    ));
    refs.extend(extract_quoted_arg(
        file,
        content,
        "os.getenv(",
        SecretSource::PythonEnvVar,
    ));
    refs.extend(extract_quoted_arg(
        file,
        content,
        "os.environ.get(",
        SecretSource::PythonEnvVar,
    ));

    refs.extend(extract_tooling_markers(file, content));
    dedup_refs(refs)
}

pub fn implied_tool_reference(file: &str) -> Option<SecretReference> {
    let lower = file.to_ascii_lowercase();
    let file_name = Path::new(file)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(file)
        .to_ascii_lowercase();

    let source = match file_name.as_str() {
        ".sops.yaml" | ".sops.yml" | "sops.yaml" | "sops.yml" => Some(SecretSource::Sops),
        ".infisical.json" | "infisical.json" | "infisical.toml" => Some(SecretSource::Infisical),
        "doppler.yaml" | "doppler.yml" | ".doppler.yaml" | ".doppler.yml" => {
            Some(SecretSource::Doppler)
        }
        ".envrc" => Some(SecretSource::Direnv),
        "mise.toml" | ".mise.toml" => Some(SecretSource::MiseEnv),
        _ if lower.ends_with("vault.hcl")
            || lower.contains("/vault/")
            || lower.contains("/openbao/") =>
        {
            Some(SecretSource::VaultOrOpenBao)
        }
        _ if lower.ends_with("docker-compose.yml")
            || lower.ends_with("docker-compose.yaml")
            || lower.ends_with("compose.yml")
            || lower.ends_with("compose.yaml") =>
        {
            Some(SecretSource::DockerComposeEnvFile)
        }
        _ => None,
    }?;

    Some(SecretReference {
        file: file.to_string(),
        key: source
            .to_string()
            .to_ascii_uppercase()
            .replace(['-', '.', '/'], "_"),
        source,
    })
}

fn extract_after_marker(
    file: &str,
    content: &str,
    marker: &str,
    source: SecretSource,
) -> Vec<SecretReference> {
    let mut refs = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = content[search_start..].find(marker) {
        let start = search_start + pos + marker.len();
        let rest = &content[start..];
        let key = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
            .collect::<String>();
        if is_env_key(&key) {
            refs.push(SecretReference {
                file: file.to_string(),
                key,
                source,
            });
        }
        search_start = start.saturating_add(1);
    }
    refs
}

fn extract_bracket_marker(
    file: &str,
    content: &str,
    marker: &str,
    source: SecretSource,
) -> Vec<SecretReference> {
    let mut refs = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = content[search_start..].find(marker) {
        let start = search_start + pos + marker.len();
        let rest = &content[start..];
        let rest = rest.trim_start();
        let quote = match rest.chars().next() {
            Some('"') => '"',
            Some('\'') => '\'',
            _ => {
                search_start = start.saturating_add(1);
                continue;
            }
        };
        let value_start = start + content[start..].find(quote).unwrap_or(0) + quote.len_utf8();
        if let Some(end_rel) = content[value_start..].find(quote) {
            let key = &content[value_start..value_start + end_rel];
            if is_env_key(key) {
                refs.push(SecretReference {
                    file: file.to_string(),
                    key: key.to_string(),
                    source,
                });
            }
            search_start = value_start + end_rel + 1;
        } else {
            search_start = start.saturating_add(1);
        }
    }
    refs
}

fn extract_quoted_arg(
    file: &str,
    content: &str,
    marker: &str,
    source: SecretSource,
) -> Vec<SecretReference> {
    let mut refs = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = content[search_start..].find(marker) {
        let call_start = search_start + pos + marker.len();
        let rest = &content[call_start..];
        let rest_trimmed = rest.trim_start();
        let quote = match rest_trimmed.chars().next() {
            Some('"') => '"',
            Some('\'') => '\'',
            _ => {
                search_start = call_start.saturating_add(1);
                continue;
            }
        };
        let leading_ws = rest.len().saturating_sub(rest_trimmed.len());
        let value_start = call_start + leading_ws + quote.len_utf8();
        if let Some(end_rel) = content[value_start..].find(quote) {
            let key = &content[value_start..value_start + end_rel];
            if is_env_key(key) {
                refs.push(SecretReference {
                    file: file.to_string(),
                    key: key.to_string(),
                    source,
                });
            }
            search_start = value_start + end_rel + 1;
        } else {
            break;
        }
    }
    refs
}

fn extract_tooling_markers(file: &str, content: &str) -> Vec<SecretReference> {
    let lower = content.to_ascii_lowercase();
    let mut refs = Vec::new();
    let markers = [
        ("infisical", SecretSource::Infisical),
        ("sops", SecretSource::Sops),
        ("vault", SecretSource::VaultOrOpenBao),
        ("openbao", SecretSource::VaultOrOpenBao),
        ("doppler", SecretSource::Doppler),
        ("env_file:", SecretSource::DockerComposeEnvFile),
    ];

    for (needle, source) in markers {
        if lower.contains(needle) {
            refs.push(SecretReference {
                file: file.to_string(),
                key: source
                    .to_string()
                    .to_ascii_uppercase()
                    .replace(['-', '.'], "_"),
                source,
            });
        }
    }
    refs
}

fn dedup_refs(refs: Vec<SecretReference>) -> Vec<SecretReference> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for r in refs {
        let key = format!("{}:{}:{}", r.file, r.key, r.source);
        if seen.insert(key) {
            out.push(r);
        }
    }
    out
}

pub fn is_env_key(key: &str) -> bool {
    if key.len() < 2 || key.len() > 128 {
        return false;
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_dotenv_keys() {
        let keys =
            extract_env_keys_from_dotenv("export API_URL=https://x\nKEY=<val>\n#COMMENT=x\n");
        assert_eq!(keys, vec!["API_URL".to_string(), "KEY".to_string()]);
    }

    #[test]
    fn extracts_polyglot_env_refs() {
        let refs = extract_secret_refs(
            "x",
            "process.env.API_KEY; process.env['DB_URL']; os.getenv(\"PY_KEY\"); import.meta.env.VITE_API_URL;",
        );
        let keys = refs.into_iter().map(|r| r.key).collect::<BTreeSet<_>>();
        assert!(keys.contains("API_KEY"));
        assert!(keys.contains("DB_URL"));
        assert!(keys.contains("PY_KEY"));
        assert!(keys.contains("VITE_API_URL"));
    }
}
