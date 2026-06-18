## MODIFIED Requirements

### Requirement: Configuration file loading
The system SHALL load configuration from `openspec/tui-config.yaml` relative to the current working directory at application startup. If the file does not exist, the system SHALL use default values.

#### Scenario: Config file exists
- **WHEN** the application starts and `openspec/tui-config.yaml` exists
- **THEN** the system SHALL parse the YAML file and use its `command`, `prompt`, `post_implementation_prompt`, `interactive_command`, and `run_finished_command` values

#### Scenario: Config file does not exist
- **WHEN** the application starts and `openspec/tui-config.yaml` does not exist
- **THEN** the system SHALL use the default command `claude --print --dangerously-skip-permissions {prompt}`, the default prompt template, and an empty `run_finished_command`

#### Scenario: Config file has partial fields
- **WHEN** the config file exists but is missing any field
- **THEN** the system SHALL use the default value for the missing field

### Requirement: Configuration file saving
The system SHALL write configuration to `openspec/tui-config.yaml` when the user saves from the Config screen.

#### Scenario: Save configuration
- **WHEN** the user presses `S` in the Config screen
- **THEN** the system SHALL write all fields including `run_finished_command` to `openspec/tui-config.yaml` in YAML format

### Requirement: Default configuration values
The system SHALL provide default values matching the current hardcoded behavior.

#### Scenario: Default run_finished_command
- **WHEN** no configuration is provided for `run_finished_command`
- **THEN** the default SHALL be an empty string (disabled)

### Requirement: Reset to defaults
The system SHALL allow resetting configuration to default values and writing those defaults to the config file.

#### Scenario: Reset to defaults
- **WHEN** the user presses `D` in the Config screen
- **THEN** the `run_finished_command` field SHALL be set to an empty string along with all other fields being reset to their defaults

## ADDED Requirements

### Requirement: Run finished command Config screen field
The system SHALL display `run_finished_command` as an inline-editable field in the Config screen.

#### Scenario: Field displayed in Config screen
- **WHEN** the user opens the Config screen
- **THEN** a `RunFinishedCommand` field SHALL be displayed showing the current value

#### Scenario: Tab navigation includes new field
- **WHEN** the user presses Tab in the Config screen
- **THEN** the focus SHALL cycle through Command → Prompt → PostImplementationPrompt → InteractiveCommand → RunFinishedCommand → Command

#### Scenario: Inline editing of run_finished_command
- **WHEN** the user presses Enter on the `RunFinishedCommand` field
- **THEN** the field SHALL enter inline edit mode (character-by-character input, same as Command field)
