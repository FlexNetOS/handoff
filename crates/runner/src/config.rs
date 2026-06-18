use std::path::Path;

use serde::{Deserialize, Serialize};

pub const CONFIG_PATH: &str = "openspec/tui-config.yaml";
const DEFAULT_COMMAND: &str = "claude --print --dangerously-skip-permissions {prompt}";
const DEFAULT_INTERACTIVE_COMMAND: &str = "claude";

/// Maximum consecutive retries for a single task before stalling (default).
const DEFAULT_RETRY_ON_FAILURE: u32 = 1;

const DEFAULT_PROMPT: &str = "Before implementing, read the following files for context:\n\
1. openspec/config.yaml — project context and conventions\n\
2. openspec/changes/{name}/proposal.md — change motivation and scope\n\
3. openspec/changes/{name}/design.md — architecture decisions\n\
4. openspec/changes/{name}/specs/ — detailed requirements\n\
5. openspec/specs/ — global project specifications\n\
\n\
Then read openspec/changes/{name}/tasks.md, take the next unfinished task, \
implement this task, verify if the changes are correct, \
and mark the task as completed.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiConfig {
    #[serde(default = "default_command")]
    pub command: String,
    #[serde(default = "default_prompt")]
    pub prompt: String,
    #[serde(default)]
    pub post_implementation_prompt: String,
    #[serde(default = "default_interactive_command")]
    pub interactive_command: String,
    #[serde(default)]
    pub run_finished_command: String,
    /// Consecutive no-progress retries per task before stalling. Defaults to 1.
    #[serde(default = "default_retry_on_failure")]
    pub retry_on_failure: u32,
}

fn default_command() -> String {
    DEFAULT_COMMAND.to_string()
}

fn default_prompt() -> String {
    DEFAULT_PROMPT.to_string()
}

fn default_interactive_command() -> String {
    DEFAULT_INTERACTIVE_COMMAND.to_string()
}

fn default_retry_on_failure() -> u32 {
    DEFAULT_RETRY_ON_FAILURE
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            command: default_command(),
            prompt: default_prompt(),
            post_implementation_prompt: String::new(),
            interactive_command: default_interactive_command(),
            run_finished_command: String::new(),
            retry_on_failure: DEFAULT_RETRY_ON_FAILURE,
        }
    }
}

impl TuiConfig {
    /// Replace `{name}` in the prompt template with the given change name.
    pub fn render_prompt(&self, name: &str) -> String {
        self.prompt.replace("{name}", name)
    }

    /// Replace `{name}` in the post-implementation prompt template.
    /// Returns `None` if the prompt is empty (no hook configured).
    pub fn render_post_prompt(&self, name: &str) -> Option<String> {
        if self.post_implementation_prompt.is_empty() {
            None
        } else {
            Some(self.post_implementation_prompt.replace("{name}", name))
        }
    }

    /// Replace `{prompt}` in the command template, split on whitespace, and
    /// return `(binary, args)`. Returns `None` if the command template is empty.
    pub fn build_command(&self, prompt: &str) -> Option<(String, Vec<String>)> {
        let parts: Vec<String> = self
            .command
            .split_whitespace()
            .map(|token| {
                if token.contains("{prompt}") {
                    token.replace("{prompt}", prompt)
                } else {
                    token.to_string()
                }
            })
            .collect();
        let (first, rest) = parts.split_first()?;
        Some((first.clone(), rest.to_vec()))
    }

    /// Split `interactive_command` on whitespace and return `(binary, args)`.
    /// Returns `None` if the command is empty.
    pub fn build_interactive_command(&self) -> Option<(String, Vec<String>)> {
        let parts: Vec<&str> = self.interactive_command.split_whitespace().collect();
        let (first, rest) = parts.split_first()?;
        Some((
            first.to_string(),
            rest.iter().map(|s| s.to_string()).collect(),
        ))
    }

    /// Load config from a specific path. Falls back to defaults if file is missing.
    pub fn load_from(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(path)?;
        let config: TuiConfig = serde_norway::from_str(&contents)?;
        Ok(config)
    }

    /// Save config to a specific path.
    pub fn save_to(&self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let yaml = serde_norway::to_string(self)?;
        std::fs::write(path, yaml)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_command() {
        let config = TuiConfig::default();
        assert_eq!(
            config.command,
            "claude --print --dangerously-skip-permissions {prompt}"
        );
    }

    #[test]
    fn test_default_prompt_contains_name_placeholder() {
        let config = TuiConfig::default();
        assert!(
            config.prompt.contains("{name}"),
            "default prompt should contain {{name}} placeholder"
        );
    }

    #[test]
    fn test_default_prompt_contains_context_references() {
        let config = TuiConfig::default();
        assert!(config.prompt.contains("openspec/config.yaml"));
        assert!(config.prompt.contains("proposal.md"));
        assert!(config.prompt.contains("design.md"));
        assert!(config.prompt.contains("specs/"));
        assert!(config.prompt.contains("tasks.md"));
    }

    #[test]
    fn test_clone() {
        let config = TuiConfig::default();
        let cloned = config.clone();
        assert_eq!(config.command, cloned.command);
        assert_eq!(config.prompt, cloned.prompt);
    }

    #[test]
    fn test_deserialize_partial_uses_defaults() {
        let yaml = "command: custom-tool {prompt}\n";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.command, "custom-tool {prompt}");
        assert_eq!(config.prompt, DEFAULT_PROMPT);
    }

    #[test]
    fn test_deserialize_empty_uses_defaults() {
        let yaml = "{}";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.command, DEFAULT_COMMAND);
        assert_eq!(config.prompt, DEFAULT_PROMPT);
    }

    #[test]
    fn test_serialize_roundtrip() {
        let config = TuiConfig {
            command: "my-tool {prompt}".to_string(),
            prompt: "do {name} stuff".to_string(),
            ..Default::default()
        };
        let yaml = serde_norway::to_string(&config).unwrap();
        let deserialized: TuiConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(config.command, deserialized.command);
        assert_eq!(config.prompt, deserialized.prompt);
        assert_eq!(
            config.post_implementation_prompt,
            deserialized.post_implementation_prompt
        );
    }

    #[test]
    fn test_serialize_roundtrip_with_post_prompt() {
        let config = TuiConfig {
            command: "my-tool {prompt}".to_string(),
            prompt: "do {name} stuff".to_string(),
            post_implementation_prompt: "commit {name}".to_string(),
            ..Default::default()
        };
        let yaml = serde_norway::to_string(&config).unwrap();
        let deserialized: TuiConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(deserialized.post_implementation_prompt, "commit {name}");
    }

    #[test]
    fn test_deserialize_without_post_prompt_defaults_to_empty() {
        let yaml = "command: my-tool {prompt}\nprompt: do stuff\n";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.post_implementation_prompt, "");
    }

    #[test]
    fn test_default_post_implementation_prompt_is_empty() {
        let config = TuiConfig::default();
        assert_eq!(config.post_implementation_prompt, "");
    }

    #[test]
    fn test_default_interactive_command() {
        let config = TuiConfig::default();
        assert_eq!(config.interactive_command, "claude");
    }

    #[test]
    fn test_deserialize_with_interactive_command() {
        let yaml = "interactive_command: aider\n";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.interactive_command, "aider");
    }

    #[test]
    fn test_deserialize_without_interactive_command_defaults_to_claude() {
        let yaml = "command: my-tool {prompt}\nprompt: do stuff\n";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.interactive_command, "claude");
    }

    #[test]
    fn test_serialize_roundtrip_with_interactive_command() {
        let config = TuiConfig {
            interactive_command: "aider --model gpt4".to_string(),
            ..Default::default()
        };
        let yaml = serde_norway::to_string(&config).unwrap();
        let deserialized: TuiConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(deserialized.interactive_command, "aider --model gpt4");
    }

    #[test]
    fn test_default_run_finished_command_is_empty() {
        let config = TuiConfig::default();
        assert_eq!(config.run_finished_command, "");
    }

    #[test]
    fn test_deserialize_with_run_finished_command() {
        let yaml = "run_finished_command: notify-send done\n";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.run_finished_command, "notify-send done");
    }

    #[test]
    fn test_deserialize_without_run_finished_command_defaults_to_empty() {
        let yaml = "command: my-tool {prompt}\nprompt: do stuff\n";
        let config: TuiConfig = serde_norway::from_str(yaml).unwrap();
        assert_eq!(config.run_finished_command, "");
    }

    #[test]
    fn test_serialize_roundtrip_with_run_finished_command() {
        let config = TuiConfig {
            run_finished_command: "ntfy pub topic done".to_string(),
            ..Default::default()
        };
        let yaml = serde_norway::to_string(&config).unwrap();
        let deserialized: TuiConfig = serde_norway::from_str(&yaml).unwrap();
        assert_eq!(deserialized.run_finished_command, "ntfy pub topic done");
    }

    /// Golden on-disk format: the serde_norway YAML output of a fully-populated
    /// config must be byte-identical to the established block-style layout
    /// (field order, no quoting of `{placeholder}` values, trailing newline).
    /// This locks `openspec/tui-config.yaml` compatibility across the
    /// serde_yaml→serde_norway migration so existing user config files keep
    /// loading and saving unchanged.
    #[test]
    fn test_golden_yaml_on_disk_format_is_stable() {
        let config = TuiConfig {
            command: "my-tool {prompt}".to_string(),
            prompt: "do {name} stuff".to_string(),
            post_implementation_prompt: "commit {name}".to_string(),
            interactive_command: "claude-i".to_string(),
            run_finished_command: "notify done".to_string(),
            retry_on_failure: 1,
        };
        let golden = "command: my-tool {prompt}\n\
                      prompt: do {name} stuff\n\
                      post_implementation_prompt: commit {name}\n\
                      interactive_command: claude-i\n\
                      run_finished_command: notify done\n";
        assert_eq!(serde_norway::to_string(&config).unwrap(), golden);
        // And it round-trips back to the same struct.
        let back: TuiConfig = serde_norway::from_str(golden).unwrap();
        assert_eq!(back.command, config.command);
        assert_eq!(back.prompt, config.prompt);
        assert_eq!(
            back.post_implementation_prompt,
            config.post_implementation_prompt
        );
        assert_eq!(back.interactive_command, config.interactive_command);
        assert_eq!(back.run_finished_command, config.run_finished_command);
    }

    mod placeholder_tests {
        use super::*;

        #[test]
        fn test_render_prompt_replaces_name() {
            let config = TuiConfig {
                command: default_command(),
                prompt: "implement {name} now".to_string(),
                ..Default::default()
            };
            assert_eq!(config.render_prompt("my-change"), "implement my-change now");
        }

        #[test]
        fn test_render_prompt_replaces_all_occurrences() {
            let config = TuiConfig {
                command: default_command(),
                prompt: "{name}/proposal.md and {name}/tasks.md".to_string(),
                ..Default::default()
            };
            assert_eq!(
                config.render_prompt("feat"),
                "feat/proposal.md and feat/tasks.md"
            );
        }

        #[test]
        fn test_render_prompt_no_placeholder() {
            let config = TuiConfig {
                command: default_command(),
                prompt: "no placeholder here".to_string(),
                ..Default::default()
            };
            assert_eq!(config.render_prompt("x"), "no placeholder here");
        }

        #[test]
        fn test_render_prompt_default_substitutes_correctly() {
            let config = TuiConfig::default();
            let rendered = config.render_prompt("add-auth");
            assert!(rendered.contains("openspec/changes/add-auth/proposal.md"));
            assert!(rendered.contains("openspec/changes/add-auth/tasks.md"));
            assert!(!rendered.contains("{name}"));
        }

        #[test]
        fn test_render_post_prompt_returns_none_when_empty() {
            let config = TuiConfig::default();
            assert!(config.render_post_prompt("anything").is_none());
        }

        #[test]
        fn test_render_post_prompt_replaces_name() {
            let config = TuiConfig {
                post_implementation_prompt: "commit changes for {name}".to_string(),
                ..Default::default()
            };
            let result = config.render_post_prompt("my-change");
            assert_eq!(result, Some("commit changes for my-change".to_string()));
        }

        #[test]
        fn test_render_post_prompt_without_placeholder() {
            let config = TuiConfig {
                post_implementation_prompt: "commit all changes".to_string(),
                ..Default::default()
            };
            let result = config.render_post_prompt("ignored");
            assert_eq!(result, Some("commit all changes".to_string()));
        }

        #[test]
        fn test_render_post_prompt_replaces_all_occurrences() {
            let config = TuiConfig {
                post_implementation_prompt: "{name} done, archive {name}".to_string(),
                ..Default::default()
            };
            let result = config.render_post_prompt("feat");
            assert_eq!(result, Some("feat done, archive feat".to_string()));
        }

        #[test]
        fn test_build_command_basic() {
            let config = TuiConfig {
                command: "claude --print {prompt}".to_string(),
                prompt: default_prompt(),
                ..Default::default()
            };
            let (bin, args) = config.build_command("do stuff").unwrap();
            assert_eq!(bin, "claude");
            assert_eq!(args, vec!["--print", "do stuff"]);
        }

        #[test]
        fn test_build_command_default() {
            let config = TuiConfig::default();
            let (bin, args) = config.build_command("hello world").unwrap();
            assert_eq!(bin, "claude");
            assert_eq!(
                args,
                vec!["--print", "--dangerously-skip-permissions", "hello world"]
            );
        }

        #[test]
        fn test_build_command_custom_tool() {
            let config = TuiConfig {
                command: "aider --message {prompt}".to_string(),
                prompt: default_prompt(),
                ..Default::default()
            };
            let (bin, args) = config.build_command("fix bug").unwrap();
            assert_eq!(bin, "aider");
            assert_eq!(args, vec!["--message", "fix bug"]);
        }

        #[test]
        fn test_build_command_no_prompt_placeholder() {
            let config = TuiConfig {
                command: "my-tool --flag --verbose".to_string(),
                prompt: default_prompt(),
                ..Default::default()
            };
            let (bin, args) = config.build_command("ignored").unwrap();
            assert_eq!(bin, "my-tool");
            assert_eq!(args, vec!["--flag", "--verbose"]);
        }

        #[test]
        fn test_build_command_empty_returns_none() {
            let config = TuiConfig {
                command: "".to_string(),
                prompt: default_prompt(),
                ..Default::default()
            };
            assert!(config.build_command("test").is_none());
        }

        #[test]
        fn test_build_command_single_binary() {
            let config = TuiConfig {
                command: "my-script".to_string(),
                prompt: default_prompt(),
                ..Default::default()
            };
            let (bin, args) = config.build_command("test").unwrap();
            assert_eq!(bin, "my-script");
            assert!(args.is_empty());
        }

        #[test]
        fn test_build_interactive_command_default() {
            let config = TuiConfig::default();
            let (bin, args) = config.build_interactive_command().unwrap();
            assert_eq!(bin, "claude");
            assert!(args.is_empty());
        }

        #[test]
        fn test_build_interactive_command_with_args() {
            let config = TuiConfig {
                interactive_command: "claude --model opus".to_string(),
                ..Default::default()
            };
            let (bin, args) = config.build_interactive_command().unwrap();
            assert_eq!(bin, "claude");
            assert_eq!(args, vec!["--model", "opus"]);
        }

        #[test]
        fn test_build_interactive_command_empty_returns_none() {
            let config = TuiConfig {
                interactive_command: "".to_string(),
                ..Default::default()
            };
            assert!(config.build_interactive_command().is_none());
        }

        #[test]
        fn test_build_interactive_command_single_binary() {
            let config = TuiConfig {
                interactive_command: "aider".to_string(),
                ..Default::default()
            };
            let (bin, args) = config.build_interactive_command().unwrap();
            assert_eq!(bin, "aider");
            assert!(args.is_empty());
        }
    }

    mod load_save_tests {
        use super::*;
        use std::fs;

        #[test]
        fn test_load_missing_file_returns_defaults() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("nonexistent.yaml");
            let config = TuiConfig::load_from(&path).unwrap();
            assert_eq!(config.command, DEFAULT_COMMAND);
            assert_eq!(config.prompt, DEFAULT_PROMPT);
        }

        #[test]
        fn test_load_full_file() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("config.yaml");
            fs::write(&path, "command: my-tool {prompt}\nprompt: do {name}\n").unwrap();
            let config = TuiConfig::load_from(&path).unwrap();
            assert_eq!(config.command, "my-tool {prompt}");
            assert_eq!(config.prompt, "do {name}");
        }

        #[test]
        fn test_load_partial_fields_uses_defaults() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("config.yaml");
            fs::write(&path, "command: custom {prompt}\n").unwrap();
            let config = TuiConfig::load_from(&path).unwrap();
            assert_eq!(config.command, "custom {prompt}");
            assert_eq!(config.prompt, DEFAULT_PROMPT);
        }

        #[test]
        fn test_save_creates_file() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("subdir").join("config.yaml");
            let config = TuiConfig {
                command: "test-tool {prompt}".to_string(),
                prompt: "test prompt {name}".to_string(),
                ..Default::default()
            };
            config.save_to(&path).unwrap();
            assert!(path.exists());
        }

        #[test]
        fn test_save_and_load_roundtrip() {
            let tmp = tempfile::tempdir().unwrap();
            let path = tmp.path().join("config.yaml");
            let config = TuiConfig {
                command: "my-cli --flag {prompt}".to_string(),
                prompt: "implement {name} please".to_string(),
                ..Default::default()
            };
            config.save_to(&path).unwrap();
            let loaded = TuiConfig::load_from(&path).unwrap();
            assert_eq!(config.command, loaded.command);
            assert_eq!(config.prompt, loaded.prompt);
        }
    }
}
