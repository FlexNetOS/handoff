## ADDED Requirements

### Requirement: Run finished command configuration
The system SHALL support a `run_finished_command` field in `tui-config.yaml` that holds an arbitrary shell command string. When the field is empty or absent, no command SHALL be executed.

#### Scenario: Config with run_finished_command set
- **WHEN** `tui-config.yaml` contains `run_finished_command: "notify-send Done"`
- **THEN** the system SHALL load and store this value in `TuiConfig`

#### Scenario: Config without run_finished_command
- **WHEN** `tui-config.yaml` does not contain `run_finished_command`
- **THEN** the system SHALL default to an empty string

#### Scenario: Config with empty run_finished_command
- **WHEN** `tui-config.yaml` contains `run_finished_command: ""`
- **THEN** the system SHALL treat it as disabled and not execute any command

### Requirement: Command execution after single run completes
The system SHALL execute `run_finished_command` once when a single implementation run finishes and no batch is active.

#### Scenario: Single run finishes successfully
- **WHEN** a single implementation run sends `Finished { success: true }` and no batch is active
- **THEN** the system SHALL spawn `run_finished_command` via `sh -c` (or `cmd /C` on Windows)

#### Scenario: Single run finishes with failure
- **WHEN** a single implementation run sends `Finished { success: false }` and no batch is active
- **THEN** the system SHALL spawn `run_finished_command`

#### Scenario: Single run stalls
- **WHEN** a single implementation run sends `Stalled` and no batch is active
- **THEN** the system SHALL spawn `run_finished_command`

### Requirement: Command execution after batch run completes
The system SHALL execute `run_finished_command` once when the entire batch is exhausted (no more changes to run).

#### Scenario: Batch run completes all changes
- **WHEN** the last change in a batch run finishes and no successor is started
- **THEN** the system SHALL spawn `run_finished_command` exactly once

#### Scenario: Batch run with failures still notifies
- **WHEN** a batch run ends with some changes failed or skipped
- **THEN** the system SHALL spawn `run_finished_command` once after the batch is fully exhausted

#### Scenario: Mid-batch change finishes
- **WHEN** a change finishes but the batch has more changes queued
- **THEN** the system SHALL NOT execute `run_finished_command`

### Requirement: Fire-and-forget execution
The system SHALL spawn the notification command without waiting for it to complete. The TUI SHALL NOT block or capture output.

#### Scenario: Command is spawned and forgotten
- **WHEN** `run_finished_command` is executed
- **THEN** the system SHALL use `std::process::Command::spawn()` and immediately discard the child handle
- **THEN** the TUI SHALL remain responsive

### Requirement: Command not executed when disabled
The system SHALL NOT attempt to spawn a process when `run_finished_command` is empty.

#### Scenario: Empty run_finished_command
- **WHEN** a run finishes and `run_finished_command` is an empty string
- **THEN** the system SHALL NOT spawn any process
