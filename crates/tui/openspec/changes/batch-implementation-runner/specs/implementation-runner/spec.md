## ADDED Requirements

### Requirement: Runner supports batch mode advancement
The system SHALL support advancing to the next change in a batch run after the current change completes. The runner SHALL notify the batch state when a change finishes (success or failure).

#### Scenario: Change completes in batch mode
- **WHEN** the current change finishes with all tasks completed during a batch run
- **THEN** the system SHALL mark the change as completed in the batch state and start the next eligible change

#### Scenario: Change fails in batch mode
- **WHEN** the current change's runner exits with failure during a batch run
- **THEN** the system SHALL mark the change as failed in the batch state, skip dependent changes, and start the next independent change

#### Scenario: Last change in batch completes
- **WHEN** the last eligible change in a batch run completes
- **THEN** the batch run SHALL be marked as finished and the batch state SHALL be cleared
