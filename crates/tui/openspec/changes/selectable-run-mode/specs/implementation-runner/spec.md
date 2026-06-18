## ADDED Requirements

### Requirement: Apply run mode launches single opsx:apply invocation
When a change's `run_mode` is `"apply"`, the runner SHALL launch a single subprocess that invokes the configured command with `/opsx:apply <change-name>` as the prompt. The runner SHALL NOT parse tasks, track progress, or detect stalls in this mode.

#### Scenario: Start apply mode run
- **WHEN** the user presses `R` on a change with `run_mode: apply`
- **THEN** the system SHALL spawn a single subprocess with `/opsx:apply <change-name>` as the prompt
- **THEN** subprocess output SHALL be redirected to `implementation.log`
- **THEN** the system SHALL navigate to the implementation log view

#### Scenario: Apply mode sends only Finished
- **WHEN** an apply mode subprocess completes
- **THEN** the runner SHALL send a `Finished` update (no intermediate `Progress` or `Stalled` messages)

#### Scenario: Apply mode supports cancellation
- **WHEN** the user presses `S` during an apply mode run
- **THEN** the subprocess SHALL be killed and the run SHALL stop

#### Scenario: Normal mode unchanged
- **WHEN** the user presses `R` on a change with `run_mode: normal` (or no `run_mode` set)
- **THEN** the system SHALL use the existing task-by-task implementation loop

### Requirement: Batch run respects per-change run mode
When running multiple changes via "Run All", each change SHALL use its own `run_mode` from `change-config.yaml`.

#### Scenario: Mixed batch run
- **WHEN** a batch contains change A with `run_mode: normal` and change B with `run_mode: apply`
- **THEN** change A SHALL run with the task-by-task loop
- **THEN** change B SHALL run with a single `opsx:apply` invocation
