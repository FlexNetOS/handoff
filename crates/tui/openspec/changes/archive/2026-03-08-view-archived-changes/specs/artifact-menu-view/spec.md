## ADDED Requirements

### Requirement: Disable implementation runner for archived changes
The system SHALL prevent starting the implementation runner when viewing an archived change.

#### Scenario: Press R on archived change artifact menu
- **WHEN** the user presses `R` on the artifact menu of an archived change
- **THEN** nothing happens and no implementation runner is started

#### Scenario: Press R on active change artifact menu
- **WHEN** the user presses `R` on the artifact menu of an active change and no implementation is running
- **THEN** the implementation runner starts as normal
