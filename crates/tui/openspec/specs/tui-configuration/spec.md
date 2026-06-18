## ADDED Requirements

### Requirement: Configuration file loading
The system SHALL load configuration from `openspec/tui-config.yaml` relative to the current working directory at application startup. If the file does not exist, the system SHALL use default values.

#### Scenario: Config file exists
- **WHEN** the application starts and `openspec/tui-config.yaml` exists
- **THEN** the system SHALL parse the YAML file and use its `command` and `prompt` values

#### Scenario: Config file does not exist
- **WHEN** the application starts and `openspec/tui-config.yaml` does not exist
- **THEN** the system SHALL use the default command `claude --print --dangerously-skip-permissions {prompt}` and the default prompt template

#### Scenario: Config file has partial fields
- **WHEN** the config file exists but is missing either `command` or `prompt`
- **THEN** the system SHALL use the default value for the missing field

### Requirement: Configuration file saving
The system SHALL write configuration to `openspec/tui-config.yaml` when the user saves from the Config screen.

#### Scenario: Save configuration
- **WHEN** the user presses `S` in the Config screen
- **THEN** the system SHALL write both `command` and `prompt` fields to `openspec/tui-config.yaml` in YAML format

### Requirement: Default configuration values
The system SHALL provide default values matching the current hardcoded behavior.

#### Scenario: Default command
- **WHEN** no configuration is provided for `command`
- **THEN** the default SHALL be `claude --print --dangerously-skip-permissions {prompt}`

#### Scenario: Default prompt
- **WHEN** no configuration is provided for `prompt`
- **THEN** the default SHALL be the current `build_prompt()` template text with `{name}` as placeholder

### Requirement: Command template placeholder substitution
The system SHALL replace `{prompt}` in the command template with the rendered prompt string.

#### Scenario: Command with {prompt} placeholder
- **WHEN** the runner executes a command template containing `{prompt}`
- **THEN** `{prompt}` SHALL be replaced with the fully rendered prompt (after `{name}` substitution)

#### Scenario: Command template whitespace splitting
- **WHEN** the runner parses a command template
- **THEN** it SHALL split on whitespace, use the first token as the binary, and the remaining tokens as arguments

### Requirement: Prompt template placeholder substitution
The system SHALL replace `{name}` in the prompt template with the current change name.

#### Scenario: Prompt with {name} placeholder
- **WHEN** the runner builds a prompt for change `my-feature`
- **THEN** all occurrences of `{name}` in the prompt template SHALL be replaced with `my-feature`

### Requirement: Reset to defaults
The system SHALL allow resetting configuration to default values and writing those defaults to the config file.

#### Scenario: Reset to defaults
- **WHEN** the user presses `D` in the Config screen
- **THEN** the command and prompt fields SHALL be set to their default values and these defaults SHALL be shown in the Config screen (not yet saved to disk until `S` is pressed)
