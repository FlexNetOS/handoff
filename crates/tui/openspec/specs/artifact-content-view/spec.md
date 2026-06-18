## ADDED Requirements

### Requirement: Render log files as plain text
The system SHALL render files with a `.log` extension as plain text, preserving all newlines and whitespace verbatim, without Markdown parsing.

#### Scenario: Log file with single newlines
- **WHEN** an artifact view displays a file ending in `.log` that contains single newlines between lines
- **THEN** each newline SHALL produce a separate line in the rendered output

#### Scenario: Log file with separator lines
- **WHEN** a `.log` file contains separator characters like `══════` or `──────`
- **THEN** the separator characters SHALL be displayed verbatim as plain text

#### Scenario: Non-log files still use Markdown rendering
- **WHEN** an artifact view displays a file not ending in `.log` (e.g., `.md`)
- **THEN** the content SHALL be parsed and rendered as Markdown with formatting and syntax highlighting

## MODIFIED Requirements

### Requirement: Display artifact content as plain text
The system SHALL display the markdown content of the selected artifact in a scrollable view with Markdown rendering and syntax highlighting. The content SHALL be parsed as Markdown and rendered with visual formatting rather than displayed as raw text. For files with `.log` extension, the system SHALL display the content as plain text without Markdown parsing.

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

#### Scenario: View implementation log
- **WHEN** the user selects Implementation Log from the artifact menu
- **THEN** the system reads `implementation.log` from the change directory and displays its content as plain text without Markdown parsing
