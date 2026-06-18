## ADDED Requirements

### Requirement: Interactive tool launch via keybinding
The system SHALL allow the user to launch the configured interactive tool by pressing `I` in the change list view on the Active tab.

#### Scenario: Launch interactive tool
- **WHEN** the user presses `I` in the change list view on the Active tab
- **THEN** the TUI SHALL suspend (leave alternate screen, disable raw mode), launch the configured `interactive_command`, and restore the TUI when the process exits

#### Scenario: I key ignored on Archived tab
- **WHEN** the user presses `I` in the change list view on the Archived tab
- **THEN** nothing happens

#### Scenario: I key ignored during running implementation
- **WHEN** the user presses `I` while an implementation is running
- **THEN** nothing happens

### Requirement: TUI suspension and restoration
The system SHALL properly suspend and restore the terminal state when launching an interactive tool.

#### Scenario: Terminal restore after interactive session
- **WHEN** the interactive process exits (user quits the tool)
- **THEN** the TUI SHALL re-enable raw mode, re-enter alternate screen, clear the screen, and reload change data

#### Scenario: Terminal restore after command failure
- **WHEN** the interactive command fails to start (e.g., command not found)
- **THEN** the TUI SHALL still restore terminal state and continue normally

### Requirement: Interactive command configuration
The system SHALL use the `interactive_command` field from `TuiConfig` to determine which command to launch.

#### Scenario: Default interactive command
- **WHEN** no `interactive_command` is configured
- **THEN** the system SHALL use `"claude"` as the default

#### Scenario: Custom interactive command
- **WHEN** `interactive_command` is set to `"aider"`
- **THEN** the system SHALL launch `aider` when the user presses `I`

#### Scenario: Command with arguments
- **WHEN** `interactive_command` is set to `"claude --model opus"`
- **THEN** the system SHALL split on whitespace, use `claude` as the binary and `--model opus` as arguments
