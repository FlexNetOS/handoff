## MODIFIED Requirements

### Requirement: Display artifact list for selected change
The system SHALL display a list of artifacts (Proposal, Design, Tasks, Specs) for the selected change. Additionally, if `implementation.log` exists in the change directory, it SHALL be shown as an entry at the bottom of the artifact list.

#### Scenario: All artifacts available
- **WHEN** the artifact menu is shown and all artifacts have status `"done"`
- **THEN** all items are displayed in normal style and are selectable

#### Scenario: Some artifacts missing
- **WHEN** the artifact menu is shown and some artifacts have status other than `"done"`
- **THEN** unavailable artifacts are displayed in a dimmed/greyed-out style

#### Scenario: Implementation log exists
- **WHEN** the artifact menu is shown and `implementation.log` exists in the change directory
- **THEN** an "Implementation Log" entry is shown at the bottom of the artifact list and is selectable

#### Scenario: Implementation log does not exist
- **WHEN** the artifact menu is shown and `implementation.log` does not exist in the change directory
- **THEN** no "Implementation Log" entry is shown in the artifact list
