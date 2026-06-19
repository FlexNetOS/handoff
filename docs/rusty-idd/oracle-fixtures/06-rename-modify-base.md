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

### Requirement: Export filename
The system SHALL name exported files using the widget set name.

#### Scenario: Filename uses set name
- **GIVEN** a widget set named "alpha"
- **WHEN** the user exports it
- **THEN** the file is named "alpha.csv"
