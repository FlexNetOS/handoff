## ADDED Requirements

### Requirement: Word wrap long lines in artifact content view
The system SHALL wrap lines that exceed the terminal width at word boundaries so that all content remains visible without horizontal truncation.

#### Scenario: Long line wraps at word boundary
- **WHEN** the artifact content contains a line longer than the terminal width
- **THEN** the line SHALL be wrapped at a word boundary and continue on the next display line

#### Scenario: Short lines remain unchanged
- **WHEN** the artifact content contains a line shorter than the terminal width
- **THEN** the line SHALL be displayed as a single line without modification

#### Scenario: Leading whitespace is preserved on wrapped lines
- **WHEN** the artifact content contains indented text (e.g., code blocks or nested lists) that wraps
- **THEN** the original leading whitespace of the source line SHALL be preserved
