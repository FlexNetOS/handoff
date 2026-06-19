# widget Specification

## Purpose
Defines how users export widget data for the rename+modify ordering fixture.
## Requirements
### Requirement: CSV export
The system SHALL allow users to export widget data as CSV.

#### Scenario: Successful CSV export
- **GIVEN** a user has saved widgets
- **WHEN** the user exports their widgets as CSV
- **THEN** the system provides a CSV file

### Requirement: Exported file naming
The system SHALL name exported files using the widget set name AND a timestamp suffix.

#### Scenario: Filename uses set name and timestamp
- **GIVEN** a widget set named "alpha"
- **WHEN** the user exports it
- **THEN** the file is named "alpha-2026.csv"

