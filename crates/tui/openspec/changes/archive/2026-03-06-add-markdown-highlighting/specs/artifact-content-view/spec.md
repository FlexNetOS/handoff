## MODIFIED Requirements

### Requirement: Display artifact content as plain text
The system SHALL display the markdown content of the selected artifact in a scrollable view with Markdown rendering and syntax highlighting. The content SHALL be parsed as Markdown and rendered with visual formatting rather than displayed as raw text.

#### Scenario: View proposal
- **WHEN** the user selects Proposal from the artifact menu
- **THEN** the system reads `proposal.md` from the change directory and displays its content with Markdown formatting

#### Scenario: View design
- **WHEN** the user selects Design from the artifact menu
- **THEN** the system reads `design.md` from the change directory and displays its content with Markdown formatting

#### Scenario: View tasks
- **WHEN** the user selects Tasks from the artifact menu
- **THEN** the system reads `tasks.md` from the change directory and displays its content with Markdown formatting

#### Scenario: View spec
- **WHEN** the user selects a spec sub-item from the artifact menu
- **THEN** the system reads `specs/<capability>/spec.md` from the change directory and displays its content with Markdown formatting
