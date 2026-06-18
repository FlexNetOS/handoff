## ADDED Requirements

### Requirement: Dependency view shows current dependencies
The system SHALL display a list of the current change's dependencies when the user navigates to the dependency view from the ArtifactMenu.

#### Scenario: Change has dependencies
- **WHEN** the user opens the dependency view for a change with dependencies `[add-api, add-user-model]`
- **THEN** the system SHALL display a selectable list showing `add-api` and `add-user-model`

#### Scenario: Change has no dependencies
- **WHEN** the user opens the dependency view for a change with no dependencies
- **THEN** the system SHALL display a message indicating no dependencies are configured

### Requirement: Add dependency via selection
The system SHALL allow the user to add a dependency by pressing `A`, which opens a list of all other active changes for selection.

#### Scenario: Add a dependency
- **WHEN** the user presses `A` in the dependency view
- **THEN** the system SHALL show a list of all active changes excluding the current change and already-added dependencies
- **WHEN** the user selects a change from the list
- **THEN** the system SHALL add it to the current change's dependencies and write `dependencies.yaml`

#### Scenario: No available changes to add
- **WHEN** the user presses `A` but all active changes are already dependencies or there are no other changes
- **THEN** the system SHALL display a message indicating no changes are available to add

### Requirement: Remove dependency via shortcut
The system SHALL allow the user to remove the currently selected dependency by pressing `D`.

#### Scenario: Remove selected dependency
- **WHEN** the user selects a dependency and presses `D`
- **THEN** the system SHALL remove the dependency from the list and update `dependencies.yaml`

#### Scenario: Remove on empty list
- **WHEN** the user presses `D` on an empty dependency list
- **THEN** nothing SHALL happen

### Requirement: Navigate back with Esc
The system SHALL return to the ArtifactMenu when the user presses `Esc` in the dependency view.

#### Scenario: Press Esc
- **WHEN** the user presses `Esc` in the dependency view
- **THEN** the system SHALL navigate back to the ArtifactMenu

### Requirement: Dependency view not available for archived changes
The system SHALL NOT show the Dependencies menu item or allow navigating to the dependency view for archived changes.

#### Scenario: Archived change artifact menu
- **WHEN** the user views the artifact menu for an archived change
- **THEN** the Dependencies menu item SHALL NOT be displayed
