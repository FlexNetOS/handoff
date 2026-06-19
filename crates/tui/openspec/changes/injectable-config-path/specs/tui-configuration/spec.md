## MODIFIED Requirements

### Requirement: Configuration file loading
The system SHALL load configuration from a configurable path at application startup. The default path SHALL be `openspec/tui-config.yaml` relative to the current working directory. If the file does not exist, the system SHALL use default values.

#### Scenario: Config file exists
- **WHEN** the application starts and the config file exists at the configured path
- **THEN** the system SHALL parse the YAML file and use its `command` and `prompt` values

#### Scenario: Config file does not exist
- **WHEN** the application starts and the config file does not exist at the configured path
- **THEN** the system SHALL use the default command `claude --print --dangerously-skip-permissions {prompt}` and the default prompt template

#### Scenario: Config file has partial fields
- **WHEN** the config file exists but is missing either `command` or `prompt`
- **THEN** the system SHALL use the default value for the missing field

### Requirement: Configuration file saving
The system SHALL write configuration to the configured path when the user saves from the Config screen.

#### Scenario: Save configuration
- **WHEN** the user presses `S` in the Config screen
- **THEN** the system SHALL write both `command` and `prompt` fields to the configured path in YAML format

## ADDED Requirements

### Requirement: Config path isolation in tests
The system SHALL support constructing the application with a custom config path so that tests do not write to the production config file.

#### Scenario: Test uses temporary config path
- **WHEN** the application is constructed with a custom config path pointing to a temporary directory
- **THEN** saving configuration SHALL write to the temporary path and SHALL NOT modify `openspec/tui-config.yaml`
