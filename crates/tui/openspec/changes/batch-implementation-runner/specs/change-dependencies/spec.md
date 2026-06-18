## ADDED Requirements

### Requirement: Dependencies stored in YAML file per change
The system SHALL support an optional `dependencies.yaml` file in each change directory (`openspec/changes/<name>/dependencies.yaml`) containing a `depends_on` list of change names.

#### Scenario: Change has dependencies
- **WHEN** a change directory contains a `dependencies.yaml` with `depends_on: [add-api, add-user-model]`
- **THEN** the system SHALL recognize that this change depends on `add-api` and `add-user-model`

#### Scenario: Change has no dependencies file
- **WHEN** a change directory does not contain a `dependencies.yaml`
- **THEN** the system SHALL treat the change as having no dependencies

#### Scenario: Empty depends_on list
- **WHEN** a change has a `dependencies.yaml` with `depends_on: []`
- **THEN** the system SHALL treat the change as having no dependencies

### Requirement: Topological sort of changes
The system SHALL sort changes in topological order based on their declared dependencies using Kahn's algorithm. Changes with no dependencies SHALL appear before changes that depend on them.

#### Scenario: Linear dependency chain
- **WHEN** change C depends on B, and B depends on A
- **THEN** the execution order SHALL be A, B, C

#### Scenario: Multiple independent roots
- **WHEN** changes A and B have no dependencies, and C depends on both
- **THEN** A and B SHALL appear before C in the execution order

#### Scenario: No dependencies declared
- **WHEN** no changes have dependencies
- **THEN** all changes SHALL be included in the sorted output in their original order

### Requirement: Cycle detection
The system SHALL detect circular dependencies and report an error listing the involved changes.

#### Scenario: Direct cycle
- **WHEN** change A depends on B and B depends on A
- **THEN** the system SHALL report a circular dependency error

#### Scenario: Indirect cycle
- **WHEN** change A depends on B, B depends on C, and C depends on A
- **THEN** the system SHALL report a circular dependency error

### Requirement: Archived changes count as fulfilled dependencies
The system SHALL treat archived changes as fulfilled when resolving dependencies. A dependency on an archived change SHALL be considered satisfied.

#### Scenario: Dependency on archived change
- **WHEN** change B depends on change A, and A exists in `openspec/changes/archive/`
- **THEN** the dependency on A SHALL be considered fulfilled

#### Scenario: Dependency on archived change with date prefix
- **WHEN** change B depends on `add-api` and the archive contains `2026-03-08-add-api`
- **THEN** the dependency SHALL be considered fulfilled by matching the suffix after the date prefix

### Requirement: Read and write dependencies
The system SHALL provide functions to read dependencies from `dependencies.yaml` and write updated dependencies back to the file.

#### Scenario: Write new dependency
- **WHEN** a dependency is added to a change that has no `dependencies.yaml`
- **THEN** the system SHALL create the file with the new dependency in the `depends_on` list

#### Scenario: Add dependency to existing file
- **WHEN** a dependency is added to a change that already has `dependencies.yaml`
- **THEN** the system SHALL append the new dependency to the existing `depends_on` list

#### Scenario: Remove dependency
- **WHEN** a dependency is removed from a change
- **THEN** the system SHALL update `dependencies.yaml` to exclude the removed dependency

#### Scenario: Remove last dependency
- **WHEN** the last dependency is removed from a change
- **THEN** the system SHALL write `dependencies.yaml` with an empty `depends_on` list or delete the file
