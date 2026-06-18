## ADDED Requirements

### Requirement: Open implementation log via L shortcut
The system SHALL open the implementation log directly when the user presses `L` in the artifact menu, provided the log file exists.

#### Scenario: Press L with existing log
- **WHEN** the user presses `L` in the artifact menu and `implementation.log` exists in the change directory
- **THEN** the system SHALL open the implementation log in the artifact view as plain text

#### Scenario: Press L without existing log
- **WHEN** the user presses `L` in the artifact menu and no `implementation.log` exists
- **THEN** nothing happens and the selection remains unchanged

#### Scenario: Press L on archived change
- **WHEN** the user presses `L` in the artifact menu for an archived change and `implementation.log` exists
- **THEN** the system SHALL open the implementation log in the artifact view as plain text

### Requirement: Auto-navigate to log view after starting runner
The system SHALL automatically navigate to the implementation log view after the user starts the implementation runner with `R`.

#### Scenario: Start runner with R
- **WHEN** the user presses `R` to start the implementation runner
- **THEN** the runner SHALL start AND the system SHALL navigate to the implementation log view showing the log as plain text

#### Scenario: Start runner preserves back navigation
- **WHEN** the user presses `R` and the system auto-navigates to the log view
- **THEN** pressing Esc SHALL return to the artifact menu with the previous selection preserved
