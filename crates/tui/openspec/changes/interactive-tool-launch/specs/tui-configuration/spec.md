## ADDED Requirements

### Requirement: Interactive command configuration field
The system SHALL support an `interactive_command` field in the configuration file for specifying the interactive tool binary.

#### Scenario: Default interactive command value
- **WHEN** no `interactive_command` is specified in the config file
- **THEN** the default SHALL be `"claude"`

#### Scenario: Config file with interactive_command
- **WHEN** the config file contains `interactive_command: "aider"`
- **THEN** the system SHALL use `"aider"` as the interactive command

#### Scenario: Partial config preserves interactive_command default
- **WHEN** the config file exists but does not include `interactive_command`
- **THEN** the system SHALL use the default `"claude"` for `interactive_command` while using the file's values for other fields
