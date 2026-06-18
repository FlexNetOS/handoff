## MODIFIED Requirements

### Requirement: Navigation mode supports field switching
The user SHALL be able to switch between fields using Tab and BackTab in navigation mode.

#### Scenario: Tab switches from Command to Prompt
- **WHEN** the user presses Tab in navigation mode with Command focused
- **THEN** the focused field changes to Prompt

#### Scenario: Tab switches from Prompt to Post-Implementation Prompt
- **WHEN** the user presses Tab in navigation mode with Prompt focused
- **THEN** the focused field changes to Post-Implementation Prompt

#### Scenario: Tab switches from Post-Implementation Prompt to Interactive Command
- **WHEN** the user presses Tab in navigation mode with Post-Implementation Prompt focused
- **THEN** the focused field changes to Interactive Command

#### Scenario: Tab switches from Interactive Command to Command
- **WHEN** the user presses Tab in navigation mode with Interactive Command focused
- **THEN** the focused field changes to Command

### Requirement: Enter activates editing for the focused field
Pressing Enter in navigation mode SHALL activate the appropriate editing mechanism for the focused field.

#### Scenario: Enter on Command field activates inline edit mode
- **WHEN** the user presses Enter in navigation mode with Command focused
- **THEN** the screen enters edit mode with the cursor at the end of the command text

#### Scenario: Enter on Prompt field opens external editor
- **WHEN** the user presses Enter in navigation mode with Prompt focused
- **THEN** the system opens `$EDITOR` with the prompt text

#### Scenario: Enter on Interactive Command field activates inline edit mode
- **WHEN** the user presses Enter in navigation mode with Interactive Command focused
- **THEN** the screen enters edit mode with the cursor at the end of the interactive command text

## ADDED Requirements

### Requirement: Interactive Command field display
The Config screen SHALL display the `interactive_command` field.

#### Scenario: Interactive Command field rendered
- **WHEN** the Config screen is displayed
- **THEN** the interactive command field SHALL be visible with its current value

#### Scenario: Reset defaults includes interactive command
- **WHEN** the user presses `D` in navigation mode
- **THEN** the interactive command field SHALL be reset to `"claude"`
