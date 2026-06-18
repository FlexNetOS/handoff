## ADDED Requirements

### Requirement: Runner detects stalled implementation
The implementation loop SHALL track the number of consecutive runs that produce no new completed tasks. If 3 consecutive runs complete without increasing the completed task count, the runner SHALL abort and send an `ImplUpdate::Stalled` message.

#### Scenario: Progress resets stall counter
- **WHEN** the runner completes a Claude invocation
- **THEN** the runner compares the current completed task count to the count before the invocation
- **THEN** if the count increased, the stall counter resets to 0

#### Scenario: No progress increments stall counter
- **WHEN** a Claude invocation finishes (with any exit code) and the completed task count has not increased
- **THEN** the stall counter increments by 1

#### Scenario: Stall threshold reached
- **WHEN** the stall counter reaches 3
- **THEN** the runner sends `ImplUpdate::Stalled` and stops the loop
- **THEN** no further Claude processes are spawned for this change

### Requirement: Stalled changes treated as failures in batch runner
The batch runner SHALL treat a stalled change the same as a failed change. Dependent changes SHALL be skipped when a change stalls.

#### Scenario: Batch propagates stall as failure
- **WHEN** a change in a batch run stalls
- **THEN** the batch runner marks it as failed
- **THEN** changes that depend on the stalled change are skipped
- **THEN** independent changes continue to execute

### Requirement: UI communicates stall to user
The TUI SHALL display a visible indication when a change stalls, distinguishing it from successful completion.

#### Scenario: Stall message shown
- **WHEN** a running implementation stalls
- **THEN** the status bar or screen displays a message indicating the change was aborted due to no progress
