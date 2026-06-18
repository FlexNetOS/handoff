## ADDED Requirements

### Requirement: Selection screen shows eligible changes
The system SHALL display a selection screen listing all active changes that have a `tasks.md` file, with checkboxes for inclusion/exclusion. All eligible changes SHALL be selected by default.

#### Scenario: Multiple eligible changes
- **WHEN** the user presses `A` on the ChangeList and there are 4 changes with `tasks.md`
- **THEN** the system SHALL show a selection screen with all 4 changes checked

#### Scenario: Some changes without tasks.md
- **WHEN** there are 5 active changes but only 3 have `tasks.md`
- **THEN** the selection screen SHALL only show the 3 eligible changes

### Requirement: Toggle individual changes with Space
The system SHALL allow the user to toggle the selection of individual changes by pressing `Space`.

#### Scenario: Deselect a change
- **WHEN** the user presses `Space` on a selected change
- **THEN** the change SHALL become deselected and show as `[ ]`

#### Scenario: Reselect a change
- **WHEN** the user presses `Space` on a deselected change
- **THEN** the change SHALL become selected and show as `[x]`

### Requirement: Show blocked state for dependent changes
The system SHALL show a blocked indicator `[~]` for changes whose dependencies are excluded from the selection. The blocked reason SHALL be displayed.

#### Scenario: Dependency excluded
- **WHEN** the user deselects change A and change C depends on A
- **THEN** change C SHALL show as `[~]` with a note indicating it depends on excluded change A

#### Scenario: Dependency re-included
- **WHEN** the user re-selects change A which was previously excluded
- **THEN** change C SHALL return to its previous selected/deselected state

### Requirement: Start batch run with Enter
The system SHALL start the batch run when the user presses `Enter`, using only the selected and non-blocked changes.

#### Scenario: Start with selections
- **WHEN** the user presses `Enter` with 3 of 4 changes selected
- **THEN** the system SHALL start a batch run with the 3 selected changes in topological order

#### Scenario: Cycle detected on start
- **WHEN** the user presses `Enter` and the selected changes contain a dependency cycle
- **THEN** the system SHALL display an error message about the circular dependency and NOT start the run

### Requirement: Cancel selection with Esc
The system SHALL return to the ChangeList when the user presses `Esc` without starting a run.

#### Scenario: Cancel selection
- **WHEN** the user presses `Esc` on the selection screen
- **THEN** the system SHALL return to the ChangeList without starting any implementation

### Requirement: Show task progress in selection
The system SHALL display the task progress `[completed/total]` next to each change in the selection screen.

#### Scenario: Progress display
- **WHEN** a change has 3 of 7 tasks completed
- **THEN** the selection screen SHALL show `[3/7]` next to the change name
