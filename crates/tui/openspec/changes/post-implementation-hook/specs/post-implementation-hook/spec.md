## ADDED Requirements

### Requirement: Post-implementation hook executes after successful implementation
The system SHALL execute a configurable post-implementation prompt after all tasks of a change have been successfully completed. The hook SHALL use the same `command` template as the implementation runner, substituting the `post_implementation_prompt` value as the prompt.

#### Scenario: Hook runs after successful implementation
- **WHEN** all tasks in a change are completed successfully
- **AND** `post_implementation_prompt` is configured (non-empty)
- **THEN** the system SHALL execute the `command` template with the rendered `post_implementation_prompt`

#### Scenario: Hook not configured
- **WHEN** all tasks in a change are completed successfully
- **AND** `post_implementation_prompt` is empty or not set
- **THEN** the system SHALL NOT execute any post-implementation hook

#### Scenario: Hook does not run on failed implementation
- **WHEN** the implementation process fails or is cancelled before all tasks are complete
- **THEN** the system SHALL NOT execute the post-implementation hook

### Requirement: Post-implementation hook supports name placeholder
The `post_implementation_prompt` SHALL support `{name}` as a placeholder, which is replaced with the current change name before execution.

#### Scenario: Name substitution in post prompt
- **WHEN** `post_implementation_prompt` contains `{name}`
- **AND** the change name is `add-auth`
- **THEN** `{name}` SHALL be replaced with `add-auth` before execution

### Requirement: Hook failure aborts execution
If the post-implementation hook process exits with a non-zero status, the system SHALL treat the change as failed.

#### Scenario: Hook failure in single mode
- **WHEN** the post-implementation hook exits with non-zero status
- **THEN** the system SHALL report the change as failed via `ImplUpdate::Finished { success: false }`

#### Scenario: Hook failure in batch mode
- **WHEN** the post-implementation hook exits with non-zero status during a batch run
- **THEN** the system SHALL report the change as failed
- **AND** dependent changes SHALL be skipped (existing failure propagation)

### Requirement: Hook output goes to implementation log
The post-implementation hook's stdout and stderr SHALL be redirected to the same implementation log file as the task output.

#### Scenario: Hook output in log
- **WHEN** the post-implementation hook runs
- **THEN** its stdout and stderr SHALL be appended to `openspec/changes/<name>/implementation.log`

### Requirement: Hook is cancellable
The post-implementation hook process SHALL respect the cancellation mechanism. When the user cancels, the hook process SHALL be killed.

#### Scenario: User cancels during hook
- **WHEN** the post-implementation hook is running
- **AND** the user presses `S` to stop
- **THEN** the hook process SHALL be killed
- **AND** the implementation SHALL be reported as not successful
