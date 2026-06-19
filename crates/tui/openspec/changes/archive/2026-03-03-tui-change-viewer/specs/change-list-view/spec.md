## ADDED Requirements

### Requirement: Display active changes on startup
The system SHALL display a list of active openspec changes retrieved via `openspec list --json` when launched.

#### Scenario: Changes exist
- **WHEN** the TUI starts and `openspec list --json` returns one or more changes
- **THEN** the system displays each change name in a selectable list

#### Scenario: No changes exist
- **WHEN** the TUI starts and `openspec list --json` returns an empty changes array
- **THEN** the system displays a message indicating no active changes were found

#### Scenario: openspec CLI not available
- **WHEN** the TUI starts and the `openspec` command is not found on PATH
- **THEN** the system displays an error message and exits

### Requirement: Navigate the change list with keyboard
The system SHALL allow the user to navigate the change list using arrow keys and j/k keys.

#### Scenario: Move selection down
- **WHEN** the user presses the down arrow or `j` key
- **THEN** the selection moves to the next change in the list

#### Scenario: Move selection up
- **WHEN** the user presses the up arrow or `k` key
- **THEN** the selection moves to the previous change in the list

#### Scenario: Selection wrapping
- **WHEN** the selection is at the last item and the user presses down
- **THEN** the selection stays at the last item

### Requirement: Select a change to view its artifacts
The system SHALL navigate to the artifact menu when the user presses Enter on a selected change.

#### Scenario: Enter on selected change
- **WHEN** the user presses Enter on a highlighted change
- **THEN** the system loads artifact status via `openspec status --change <name> --json` and shows the artifact menu

### Requirement: Quit from change list
The system SHALL exit when the user presses `q` from the change list screen.

#### Scenario: Quit
- **WHEN** the user presses `q` on the change list screen
- **THEN** the TUI exits and restores the terminal to its original state
