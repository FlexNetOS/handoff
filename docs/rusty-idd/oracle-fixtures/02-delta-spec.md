## RENAMED Requirements
- FROM: Export filename
- TO: Exported file naming

## REMOVED Requirements
### Requirement: Legacy XML export

Reason: XML format is deprecated.

## MODIFIED Requirements
### Requirement: Export rate limit
Limit exports to 20 per hour.

#### Scenario: Under the limit
The user has exported 19 times without error.

#### Scenario: Over the limit
The user is blocked after 20 exports.

## ADDED Requirements
### Requirement: JSON export
Export data as JSON.
