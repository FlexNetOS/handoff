## MODIFIED Requirements

### Requirement: Display active changes on startup
The system SHALL display a list of active openspec changes retrieved via `openspec list --json` when launched, with a tab indicator showing the current view.

#### Scenario: Changes exist
- **WHEN** the TUI starts and `openspec list --json` returns one or more changes
- **THEN** the system displays each change name in a selectable list with the title showing `OpenSpec TUI [Active | Archived]` with Active highlighted

#### Scenario: No changes exist
- **WHEN** the TUI starts and `openspec list --json` returns an empty changes array
- **THEN** the system displays a message indicating no active changes were found

#### Scenario: openspec CLI not available
- **WHEN** the TUI starts and the `openspec` command is not found on PATH
- **THEN** the system displays an error message and exits

## ADDED Requirements

### Requirement: Switch between Active and Archived tabs
The system SHALL allow the user to switch between Active and Archived change lists using Left/Right arrow keys or h/l keys.

#### Scenario: Switch to Archived tab
- **WHEN** the user presses the Right arrow or `l` key on the Active tab
- **THEN** the system switches to the Archived tab, loads archived changes, resets selection to 0, and updates the title to highlight Archived

#### Scenario: Switch to Active tab
- **WHEN** the user presses the Left arrow or `h` key on the Archived tab
- **THEN** the system switches to the Active tab, loads active changes, resets selection to 0, and updates the title to highlight Active

#### Scenario: Already on leftmost tab
- **WHEN** the user presses Left or `h` on the Active tab
- **THEN** nothing happens (no wrap-around)

#### Scenario: Already on rightmost tab
- **WHEN** the user presses Right or `l` on the Archived tab
- **THEN** nothing happens (no wrap-around)
