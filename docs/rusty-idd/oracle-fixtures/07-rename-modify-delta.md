## MODIFIED Requirements

### Requirement: Exported file naming
The system SHALL name exported files using the widget set name AND a timestamp suffix.

#### Scenario: Filename uses set name and timestamp
- **GIVEN** a widget set named "alpha"
- **WHEN** the user exports it
- **THEN** the file is named "alpha-2026.csv"

## RENAMED Requirements

- FROM: `### Requirement: Export filename`
- TO: `### Requirement: Exported file naming`
