## MODIFIED Requirements

### Requirement: Status bar shows progress information
The status bar SHALL display: the change name, completed/total task counts, and a visual progress bar. During a batch run, the status bar SHALL additionally show the overall batch progress.

#### Scenario: Progress display with partial completion (single run)
- **WHEN** 3 of 7 tasks are completed in a single change run
- **THEN** the status bar shows the change name, "3/7", and a progress bar at approximately 42%

#### Scenario: Progress display at zero (single run)
- **WHEN** 0 of 5 tasks are completed in a single change run
- **THEN** the status bar shows "0/5" and an empty progress bar

#### Scenario: Batch run progress display
- **WHEN** a batch run is active, currently on change 2 of 4, with task progress 3/7 on the current change
- **THEN** the status bar SHALL show the current change name, "3/7" task progress, and "Change 2/4" for overall batch progress

#### Scenario: Batch run with failures
- **WHEN** a batch run has 1 failed and 2 skipped changes
- **THEN** the status bar SHALL include indicators showing the failed and skipped counts (e.g., "1 failed, 2 skipped")
