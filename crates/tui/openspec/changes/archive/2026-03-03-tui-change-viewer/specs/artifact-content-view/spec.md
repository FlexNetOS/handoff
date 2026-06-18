## ADDED Requirements

### Requirement: Display artifact content as plain text
The system SHALL display the raw markdown content of the selected artifact in a scrollable view.

#### Scenario: View proposal
- **WHEN** the user selects Proposal from the artifact menu
- **THEN** the system reads `proposal.md` from the change directory and displays its content

#### Scenario: View design
- **WHEN** the user selects Design from the artifact menu
- **THEN** the system reads `design.md` from the change directory and displays its content

#### Scenario: View tasks
- **WHEN** the user selects Tasks from the artifact menu
- **THEN** the system reads `tasks.md` from the change directory and displays its content

#### Scenario: View spec
- **WHEN** the user selects a spec sub-item from the artifact menu
- **THEN** the system reads `specs/<capability>/spec.md` from the change directory and displays its content

### Requirement: Scroll through artifact content
The system SHALL allow vertical scrolling through the artifact content.

#### Scenario: Scroll down
- **WHEN** the user presses the down arrow or `j` key in the content view
- **THEN** the view scrolls down by one line

#### Scenario: Scroll up
- **WHEN** the user presses the up arrow or `k` key in the content view
- **THEN** the view scrolls up by one line

#### Scenario: Top boundary
- **WHEN** the view is at the top and the user presses up
- **THEN** the view stays at the top

### Requirement: Return to artifact menu from content view
The system SHALL return to the artifact menu when the user presses Esc from the content view.

#### Scenario: Press Esc
- **WHEN** the user presses Esc in the content view
- **THEN** the system returns to the artifact menu with the previous selection preserved

### Requirement: Quit from content view
The system SHALL exit when the user presses `q` from the content view.

#### Scenario: Quit
- **WHEN** the user presses `q` in the content view
- **THEN** the TUI exits and restores the terminal to its original state
