## MODIFIED Requirements

### Requirement: Scroll through artifact content
The system SHALL allow vertical scrolling through the artifact content. Scrolling SHALL operate on rendered lines (including wrapped continuation lines).

#### Scenario: Scroll down
- **WHEN** the user presses the down arrow or `j` key in the content view
- **THEN** the view scrolls down by one rendered line

#### Scenario: Scroll up
- **WHEN** the user presses the up arrow or `k` key in the content view
- **THEN** the view scrolls up by one rendered line

#### Scenario: Top boundary
- **WHEN** the view is at the top and the user presses up
- **THEN** the view stays at the top
