## ADDED Requirements

### Requirement: List archived changes from filesystem
The system SHALL read archived changes from the `openspec/changes/archive/` directory.

#### Scenario: Archive directory contains changes
- **WHEN** the user switches to the Archived tab
- **THEN** the system lists all subdirectories of `openspec/changes/archive/` as change entries with task progress parsed from each change's `tasks.md`

#### Scenario: Archive directory is empty
- **WHEN** the user switches to the Archived tab and the archive directory contains no subdirectories
- **THEN** the system displays a message indicating no archived changes were found

#### Scenario: Archive directory does not exist
- **WHEN** the user switches to the Archived tab and `openspec/changes/archive/` does not exist
- **THEN** the system displays an empty list with no error

### Requirement: Sort archived changes by date descending then name ascending
The system SHALL sort archived changes with newest dates first, and alphabetically within the same date.

#### Scenario: Multiple dates
- **WHEN** archived changes have different date prefixes (e.g., `2026-03-06-foo`, `2026-03-03-bar`)
- **THEN** the change with the later date (`2026-03-06-foo`) appears before the earlier date (`2026-03-03-bar`)

#### Scenario: Same date different names
- **WHEN** multiple archived changes share the same date prefix (e.g., `2026-03-03-add-nix-flake`, `2026-03-03-tui-change-viewer`)
- **THEN** they are sorted alphabetically by the name portion after the date prefix (`add-nix-flake` before `tui-change-viewer`)

### Requirement: Detect archived artifact availability from filesystem
The system SHALL determine artifact availability for archived changes by checking file existence rather than using `openspec status`.

#### Scenario: Artifact files exist
- **WHEN** an archived change directory contains `proposal.md`, `design.md`, `tasks.md`
- **THEN** those artifacts are shown as available in the artifact menu

#### Scenario: Artifact files missing
- **WHEN** an archived change directory is missing an artifact file (e.g., no `design.md`)
- **THEN** that artifact is shown as unavailable (greyed out) in the artifact menu

#### Scenario: Specs directory exists with subdirectories
- **WHEN** an archived change has a `specs/` directory with capability subdirectories containing `spec.md`
- **THEN** the spec sub-items are listed under the Specs header in the artifact menu

### Requirement: Resolve archive path for change directory
The system SHALL resolve change directories to `openspec/changes/archive/<name>/` for archived changes.

#### Scenario: Open artifact from archived change
- **WHEN** the user selects an artifact from an archived change's artifact menu
- **THEN** the system reads the file from `openspec/changes/archive/<name>/<artifact-file>`
