## ADDED Requirements

### Requirement: Dependencies menu item in ArtifactMenu
The system SHALL display a "Dependencies" menu item in the ArtifactMenu for active changes. The item SHALL show the number of dependencies in brackets.

#### Scenario: Change with dependencies
- **WHEN** the user opens the ArtifactMenu for an active change with 2 dependencies
- **THEN** the menu SHALL include an item labeled "Dependencies [2]"

#### Scenario: Change without dependencies
- **WHEN** the user opens the ArtifactMenu for an active change with no dependencies
- **THEN** the menu SHALL include an item labeled "Dependencies [0]"

#### Scenario: Select Dependencies item
- **WHEN** the user selects the Dependencies menu item and presses Enter
- **THEN** the system SHALL navigate to the dependency management view for that change

#### Scenario: Archived change has no Dependencies item
- **WHEN** the user opens the ArtifactMenu for an archived change
- **THEN** the Dependencies menu item SHALL NOT appear in the menu
