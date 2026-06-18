## ADDED Requirements

### Requirement: Display dependencies inline in ChangeList
The system SHALL display dependency information next to each change in the ChangeList. Changes with dependencies SHALL show their dependency names right-aligned.

#### Scenario: Change with dependencies
- **WHEN** a change has dependencies `[add-api, add-user-model]`
- **THEN** the ChangeList SHALL display `<- add-api, add-user-model` to the right of the change's progress

#### Scenario: Change without dependencies
- **WHEN** a change has no dependencies
- **THEN** no dependency information SHALL be displayed for that change

#### Scenario: Long dependency list truncation
- **WHEN** a change has more dependencies than fit in the available width
- **THEN** the system SHALL truncate with `...` to stay within terminal width

### Requirement: Run All keybinding on ChangeList
The system SHALL open the Run All selection screen when the user presses `A` on the Active tab of the ChangeList.

#### Scenario: Press A on Active tab
- **WHEN** the user presses `A` on the Active tab of the ChangeList
- **THEN** the system SHALL navigate to the Run All selection screen

#### Scenario: Press A on Archived tab
- **WHEN** the user presses `A` on the Archived tab
- **THEN** nothing SHALL happen (batch run only applies to active changes)

#### Scenario: Press A while implementation running
- **WHEN** the user presses `A` while an implementation is already running
- **THEN** nothing SHALL happen

### Requirement: Dependency graph view keybinding
The system SHALL open a dependency graph view when the user presses `G` on the Active tab of the ChangeList.

#### Scenario: Press G on Active tab
- **WHEN** the user presses `G` on the Active tab
- **THEN** the system SHALL navigate to a read-only dependency graph view

#### Scenario: Press G on Archived tab
- **WHEN** the user presses `G` on the Archived tab
- **THEN** nothing SHALL happen

### Requirement: Dependency graph visualization
The system SHALL display an ASCII tree showing all active changes and their dependency relationships.

#### Scenario: Linear dependency chain
- **WHEN** changes A -> B -> C exist (B depends on A, C depends on B)
- **THEN** the graph SHALL show A at the root with B as child and C as grandchild

#### Scenario: No dependencies
- **WHEN** no changes have dependencies
- **THEN** the graph SHALL list all changes without tree connectors

#### Scenario: Navigate back from graph
- **WHEN** the user presses `Esc` on the dependency graph view
- **THEN** the system SHALL return to the ChangeList
