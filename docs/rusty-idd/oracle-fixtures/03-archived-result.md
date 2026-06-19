# widget-export Specification

## Purpose
Defines how users export widget data.
## Requirements
### Requirement: CSV export
The system SHALL allow users to export widget data as CSV.

#### Scenario: Successful CSV export
- **GIVEN** a user has saved widgets
- **WHEN** the user exports their widgets as CSV
- **THEN** the system provides a CSV file containing the widgets

### Requirement: Export rate limit
The system SHALL limit exports to 20 per hour.

#### Scenario: Within limit
- **GIVEN** a user has exported 19 times this hour
- **WHEN** the user requests another export
- **THEN** the export succeeds

#### Scenario: Over limit
- **GIVEN** a user has exported 20 times this hour
- **WHEN** the user requests another export
- **THEN** the system rejects the export with a rate-limit error

### Requirement: Exported file naming
The system SHALL name exported files using the widget set name.

#### Scenario: Filename uses set name
- **GIVEN** a widget set named "alpha"
- **WHEN** the user exports it
- **THEN** the file is named "alpha.csv"

### Requirement: JSON export
The system SHALL allow users to export widget data as JSON.

#### Scenario: Successful JSON export
- **GIVEN** a user has saved widgets
- **WHEN** the user exports their widgets as JSON
- **THEN** the system provides a JSON file containing the widgets

