## ADDED Requirements

### Requirement: Batch execution runs changes sequentially
The system SHALL execute selected changes one at a time in topological order. Each change SHALL complete (all tasks finished) before the next change starts.

#### Scenario: Three changes with linear deps
- **WHEN** the user starts a batch run with changes A -> B -> C (A has no deps, B depends on A, C depends on B)
- **THEN** the system SHALL execute A first, then B, then C

#### Scenario: Change completes successfully
- **WHEN** change A finishes with all tasks completed
- **THEN** the system SHALL immediately start the next eligible change

### Requirement: Skip already completed changes
The system SHALL skip changes where all tasks are already marked as completed in `tasks.md`.

#### Scenario: All tasks complete
- **WHEN** a change in the batch has all tasks checked off
- **THEN** the system SHALL skip it and move to the next change

#### Scenario: Mix of completed and incomplete
- **WHEN** a batch contains changes A (complete), B (incomplete), C (incomplete)
- **THEN** the system SHALL skip A and start with B

### Requirement: Failure propagation through dependency graph
When a change fails, the system SHALL mark all transitively dependent changes as skipped and continue with independent changes.

#### Scenario: Failed change with dependents
- **WHEN** change A fails and changes C, D depend on A, while B is independent
- **THEN** C and D SHALL be marked as skipped, and B SHALL still execute

#### Scenario: Transitive skip
- **WHEN** change A fails, C depends on A, and F depends on C
- **THEN** both C and F SHALL be marked as skipped

#### Scenario: Independent change unaffected
- **WHEN** change A fails and change B has no dependency on A (directly or transitively)
- **THEN** B SHALL execute normally

### Requirement: Batch state tracking
The system SHALL maintain a `BatchImplState` tracking the queue of changes, current index, and sets of failed, skipped, and completed changes.

#### Scenario: Batch progress tracking
- **WHEN** a batch run is in progress with 5 changes, 2 completed, 1 failed, 1 skipped
- **THEN** the batch state SHALL reflect current_index, and the failed/skipped/completed sets

### Requirement: Only one batch or single run at a time
The system SHALL NOT allow starting a batch run while a single or batch implementation is already running.

#### Scenario: Implementation already running
- **WHEN** the user presses `A` for Run All while an implementation is running
- **THEN** the system SHALL not start a new batch run

### Requirement: Batch run cancellation
The system SHALL allow the user to cancel a batch run by pressing `S`, which stops the current change and cancels remaining changes.

#### Scenario: Cancel during batch run
- **WHEN** the user presses `S` during a batch run
- **THEN** the currently running change SHALL be stopped and no further changes SHALL be started
