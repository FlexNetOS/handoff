## ADDED Requirements

### Requirement: Log file stored in change directory
The runner SHALL write the implementation log to `openspec/changes/<name>/implementation.log` instead of a temporary directory.

#### Scenario: Log file created on first run
- **WHEN** the user starts an implementation run for a change
- **THEN** the file `openspec/changes/<name>/implementation.log` is created (or appended to if it already exists)

#### Scenario: Log persists across runs
- **WHEN** the user starts a second implementation run for the same change
- **THEN** the new output is appended to the existing `implementation.log` file

### Requirement: Run header written at start of each run
The runner SHALL write a visually distinct header block into the log when a new implementation run starts, containing a separator line, the text "IMPLEMENTATION RUN STARTED", a timestamp, and the change name.

#### Scenario: Run header format
- **WHEN** a new implementation run starts
- **THEN** the log contains a header block with `══` separator lines, the text "IMPLEMENTATION RUN STARTED", the current date/time, and the change name

### Requirement: Task header written before each task
The runner SHALL write a task header into the log before spawning claude for each task, containing the task number (current/total) and the task description text.

#### Scenario: Task header with task text
- **WHEN** the runner is about to execute the next unchecked task
- **THEN** the log contains a line with `──` separators, the task number as `N/M`, and the task description text extracted from tasks.md

### Requirement: Parse next unchecked task from tasks.md
The system SHALL provide a function that reads tasks.md and returns the 1-based index and description text of the first unchecked task (`- [ ]`).

#### Scenario: Tasks with mixed completion
- **WHEN** tasks.md contains `- [x] Done task` followed by `- [ ] Next task`
- **THEN** the function returns index 2 and text "Next task"

#### Scenario: All tasks complete
- **WHEN** tasks.md contains only checked tasks
- **THEN** the function returns None

#### Scenario: No tasks file
- **WHEN** tasks.md does not exist
- **THEN** the function returns an error
