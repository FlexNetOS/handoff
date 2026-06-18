## ADDED Requirements

### Requirement: Status bar visible during implementation
The system SHALL display a 2-line status bar at the bottom of the terminal while an implementation is running. The status bar SHALL be visible on all screens (ChangeList, ArtifactMenu, ArtifactView).

#### Scenario: Implementation starts
- **WHEN** a background implementation is started
- **THEN** a status bar appears at the bottom of the screen
- **THEN** the main content area shrinks by 2 lines to accommodate it

#### Scenario: Implementation ends
- **WHEN** the implementation completes or is stopped
- **THEN** the status bar disappears
- **THEN** the main content area reclaims the full terminal height

### Requirement: Status bar shows progress information
The status bar SHALL display: the change name, completed/total task counts, and a visual progress bar.

#### Scenario: Progress display with partial completion
- **WHEN** 3 of 7 tasks are completed
- **THEN** the status bar shows the change name, "3/7", and a progress bar at approximately 42%

#### Scenario: Progress display at zero
- **WHEN** 0 of 5 tasks are completed
- **THEN** the status bar shows "0/5" and an empty progress bar

### Requirement: Status bar shows log file path
The status bar SHALL display the path to the Claude output log file so the user can find it for debugging.

#### Scenario: Log path visible
- **WHEN** implementation is running
- **THEN** the log file path is shown in the status bar

### Requirement: Status bar shows stop hint
The status bar SHALL show a hint that the user can press `S` to stop the implementation.

#### Scenario: Stop hint visible
- **WHEN** implementation is running
- **THEN** the text "[S] Stop" is visible in the status bar

### Requirement: Event loop supports background updates
The main event loop SHALL use polling (not blocking reads) so it can check for progress updates from the worker thread and re-render the status bar.

#### Scenario: Progress bar updates without user input
- **WHEN** the worker completes a task
- **THEN** the status bar updates within 500ms even if no key is pressed
