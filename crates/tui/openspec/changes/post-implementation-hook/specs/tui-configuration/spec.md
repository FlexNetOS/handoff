## MODIFIED Requirements

### Requirement: Configuration file loading
The system SHALL load configuration from `openspec/tui-config.yaml` relative to the current working directory at application startup. If the file does not exist, the system SHALL use default values.

#### Scenario: Config file exists
- **WHEN** the application starts and `openspec/tui-config.yaml` exists
- **THEN** the system SHALL parse the YAML file and use its `command`, `prompt`, and `post_implementation_prompt` values

#### Scenario: Config file does not exist
- **WHEN** the application starts and `openspec/tui-config.yaml` does not exist
- **THEN** the system SHALL use the default command `claude --print --dangerously-skip-permissions {prompt}`, the default prompt template, and an empty `post_implementation_prompt`

#### Scenario: Config file has partial fields
- **WHEN** the config file exists but is missing either `command`, `prompt`, or `post_implementation_prompt`
- **THEN** the system SHALL use the default value for the missing field

### Requirement: Configuration file saving
The system SHALL write configuration to `openspec/tui-config.yaml` when the user saves from the Config screen.

#### Scenario: Save configuration
- **WHEN** the user presses `S` in the Config screen
- **THEN** the system SHALL write `command`, `prompt`, and `post_implementation_prompt` fields to `openspec/tui-config.yaml` in YAML format

### Requirement: Default configuration values
The system SHALL provide default values matching the current hardcoded behavior.

#### Scenario: Default command
- **WHEN** no configuration is provided for `command`
- **THEN** the default SHALL be `claude --print --dangerously-skip-permissions {prompt}`

#### Scenario: Default prompt
- **WHEN** no configuration is provided for `prompt`
- **THEN** the default SHALL be the current `build_prompt()` template text with `{name}` as placeholder

#### Scenario: Default post_implementation_prompt
- **WHEN** no configuration is provided for `post_implementation_prompt`
- **THEN** the default SHALL be an empty string (no hook runs)
