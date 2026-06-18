# Export Capability

## Purpose
File export capability.
## Requirements
### Requirement: CSV export
Export data as CSV.

### Requirement: Export rate limit
Limit exports to 20 per hour.

#### Scenario: Under the limit
The user has exported 19 times without error.

#### Scenario: Over the limit
The user is blocked after 20 exports.

### Requirement: Exported file naming
The filename includes a datestamp.

### Requirement: JSON export
Export data as JSON.

