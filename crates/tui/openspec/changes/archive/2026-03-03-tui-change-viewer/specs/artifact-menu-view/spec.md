## ADDED Requirements

### Requirement: Display artifact list for selected change
The system SHALL display a list of artifacts (Proposal, Design, Tasks, Specs) for the selected change.

#### Scenario: All artifacts available
- **WHEN** the artifact menu is shown and all artifacts have status `"done"`
- **THEN** all items are displayed in normal style and are selectable

#### Scenario: Some artifacts missing
- **WHEN** the artifact menu is shown and some artifacts have status other than `"done"`
- **THEN** unavailable artifacts are displayed in a dimmed/greyed-out style

### Requirement: Greyed-out artifacts are not selectable
The system SHALL prevent navigation into artifacts that are not yet created.

#### Scenario: Enter on greyed-out artifact
- **WHEN** the user presses Enter on a greyed-out artifact
- **THEN** nothing happens and the selection remains on the same item

### Requirement: Display spec sub-items under Specs entry
The system SHALL list individual spec files as sub-items under the Specs entry when specs are available.

#### Scenario: Change has specs
- **WHEN** the change has a `specs/` directory with one or more capability subdirectories
- **THEN** each capability is listed as a sub-item under Specs (e.g., `task-period-tracking`)

#### Scenario: Change has no specs
- **WHEN** the change has no `specs/` directory or it is empty
- **THEN** the Specs entry is shown greyed out with no sub-items

### Requirement: Navigate artifact menu with keyboard
The system SHALL allow keyboard navigation of the artifact menu using arrow keys and j/k keys.

#### Scenario: Navigate items
- **WHEN** the user presses up/down arrow or j/k
- **THEN** the selection moves between artifact items (including spec sub-items)

### Requirement: Go back to change list
The system SHALL return to the change list when the user presses Esc from the artifact menu.

#### Scenario: Press Esc
- **WHEN** the user presses Esc on the artifact menu
- **THEN** the system returns to the change list screen with the previous selection preserved
