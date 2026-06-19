## ADDED Requirements

### Requirement: User can refresh the current screen with `r`
The system SHALL reload the current screen's data when the user presses the `r` key. The refresh SHALL apply to all screens except the Config screen. The Config screen SHALL ignore the `r` key for refresh purposes.

#### Scenario: Refresh ChangeList (Active tab)
- **WHEN** the user is on the ChangeList screen with the Active tab selected and presses `r`
- **THEN** the system SHALL reload the change list from the CLI and reload change dependencies, preserving the current selection index

#### Scenario: Refresh ChangeList (Archived tab)
- **WHEN** the user is on the ChangeList screen with the Archived tab selected and presses `r`
- **THEN** the system SHALL reload the archived change list from the filesystem, preserving the current selection index

#### Scenario: Refresh ArtifactMenu
- **WHEN** the user is on the ArtifactMenu screen and presses `r`
- **THEN** the system SHALL reload the change status and rediscover available artifacts, preserving the current selection index

#### Scenario: Refresh ArtifactView
- **WHEN** the user is on the ArtifactView screen and presses `r`
- **THEN** the system SHALL re-read the artifact file content from disk, preserving the current scroll position

#### Scenario: Refresh DependencyView
- **WHEN** the user is on the DependencyView screen and presses `r`
- **THEN** the system SHALL reload the dependencies from the filesystem, preserving the current selection index

#### Scenario: Refresh DependencyGraph
- **WHEN** the user is on the DependencyGraph screen and presses `r`
- **THEN** the system SHALL regenerate the dependency graph from freshly loaded change data, preserving the current scroll position

#### Scenario: Refresh RunAllSelection
- **WHEN** the user is on the RunAllSelection screen and presses `r`
- **THEN** the system SHALL rebuild the run-all entries from a fresh change list, preserving the current selection index

#### Scenario: Refresh DependencyAdd
- **WHEN** the user is on the DependencyAdd screen and presses `r`
- **THEN** the system SHALL reload the list of available changes, preserving the current selection index

### Requirement: Selection index is clamped after refresh
The system SHALL preserve the user's selection index after a refresh. If the new data has fewer items than before, the selection index SHALL be clamped to the last item in the new list.

#### Scenario: List shrinks after refresh
- **WHEN** the user has item 5 selected in a list of 10, and after refresh the list contains only 3 items
- **THEN** the selection index SHALL be set to 2 (the last item, zero-indexed)

#### Scenario: List stays same size or grows
- **WHEN** the user has item 3 selected and the list size stays the same or grows after refresh
- **THEN** the selection index SHALL remain at 3

### Requirement: ArtifactView stores file path
The ArtifactView screen variant SHALL store the file path of the currently displayed artifact so that the content can be re-read on refresh.

#### Scenario: ArtifactView constructed with file path
- **WHEN** an ArtifactView is created for an artifact
- **THEN** the screen state SHALL include the file path used to read the content
