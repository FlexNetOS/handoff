## ADDED Requirements

### Requirement: Start implementation from ArtifactMenu
The system SHALL allow the user to start an implementation runner by pressing `R` on the ArtifactMenu screen. The runner SHALL spawn a background thread that iterates through unfinished tasks in the change's `tasks.md`.

#### Scenario: User presses R to start implementation
- **WHEN** user presses `R` on the ArtifactMenu screen
- **THEN** a background implementation runner starts for the currently selected change
- **THEN** the TUI remains responsive and usable

#### Scenario: User presses R while implementation already running
- **WHEN** an implementation is already running
- **WHEN** user presses `R`
- **THEN** the keypress is ignored and no second runner starts

### Requirement: Implementation loop executes Claude per task
The runner SHALL execute `claude --print --dangerously-skip-permissions` once per unfinished task. Each invocation SHALL receive a prompt instructing Claude to read tasks.md, pick the next unfinished task, implement it, verify correctness, and mark it as completed.

#### Scenario: Multiple unfinished tasks
- **WHEN** the runner starts with 3 unfinished tasks in tasks.md
- **THEN** Claude is invoked up to 3 times sequentially
- **THEN** each invocation processes one task

#### Scenario: All tasks completed
- **WHEN** no unfinished tasks remain in tasks.md (no `- [ ]` lines)
- **THEN** the runner stops and the implementation state is cleared

### Requirement: Claude output redirected to log file
The runner SHALL redirect all Claude stdout and stderr output to a log file. The log file path SHALL be `<temp_dir>/openspec-implement-<change-name>.log`.

#### Scenario: Claude produces output
- **WHEN** Claude writes to stdout during implementation
- **THEN** the output is appended to the log file
- **THEN** no output appears in the terminal

### Requirement: Stop implementation
The user SHALL be able to stop a running implementation by pressing `S` from any screen. Stopping SHALL kill the active Claude child process and end the worker thread.

#### Scenario: User stops running implementation
- **WHEN** an implementation is running
- **WHEN** user presses `S`
- **THEN** the active Claude process is killed
- **THEN** the worker thread terminates
- **THEN** the status bar disappears

#### Scenario: User presses S with no implementation running
- **WHEN** no implementation is running
- **WHEN** user presses `S`
- **THEN** the keypress is ignored

### Requirement: Progress tracking via tasks.md
The runner SHALL track progress by counting `- [x]` (completed) and `- [ ]` (uncompleted) lines in tasks.md after each Claude iteration.

#### Scenario: Progress updates after task completion
- **WHEN** Claude completes a task and marks it `[x]` in tasks.md
- **THEN** the runner reads tasks.md and sends updated counts to the TUI
