## ADDED Requirements

### Requirement: Config screen accessibility
The Config screen SHALL be accessible from any screen by pressing `C`.

#### Scenario: Open config from ChangeList
- **WHEN** the user presses `C` in the ChangeList screen
- **THEN** the Config screen SHALL be displayed with the current configuration values

#### Scenario: Open config from ArtifactMenu
- **WHEN** the user presses `C` in the ArtifactMenu screen
- **THEN** the Config screen SHALL be displayed with the current configuration values

#### Scenario: Open config from ArtifactView
- **WHEN** the user presses `C` in the ArtifactView screen
- **THEN** the Config screen SHALL be displayed with the current configuration values

### Requirement: Config screen layout
The Config screen SHALL display two configuration fields: command and prompt.

#### Scenario: Screen rendering
- **WHEN** the Config screen is displayed
- **THEN** it SHALL show the command field as an editable single-line text input and the prompt field as a multi-line preview with an indicator that Enter opens the external editor

### Requirement: Inline command editing
The Config screen SHALL support inline text editing for the command field.

#### Scenario: Type characters
- **WHEN** the command field is focused and the user types a character
- **THEN** the character SHALL be inserted at the cursor position

#### Scenario: Cursor navigation
- **WHEN** the command field is focused
- **THEN** Left/Right arrow keys SHALL move the cursor, Home/End SHALL jump to start/end, Backspace SHALL delete the character before the cursor, and Delete SHALL delete the character at the cursor

### Requirement: External editor for prompt
The Config screen SHALL open `$EDITOR` (falling back to `vi`) for editing the prompt field.

#### Scenario: Open editor
- **WHEN** the prompt field is focused and the user presses Enter
- **THEN** the system SHALL write the current prompt to a temporary file, open `$EDITOR` with that file, and after the editor exits, read the file contents back as the new prompt value

#### Scenario: EDITOR not set
- **WHEN** `$EDITOR` is not set and the user presses Enter on the prompt field
- **THEN** the system SHALL fall back to `vi`

### Requirement: Field navigation
The user SHALL be able to switch focus between the command and prompt fields.

#### Scenario: Tab switches fields
- **WHEN** the user presses Tab in the Config screen
- **THEN** the focus SHALL move to the next field (command → prompt → command)

### Requirement: Config screen actions
The Config screen SHALL support Save, Cancel, and Reset to defaults actions.

#### Scenario: Save with S
- **WHEN** the user presses `S`
- **THEN** the current field values SHALL be saved to the config file and applied to the running application, and the screen SHALL return to the previous screen

#### Scenario: Cancel with Esc
- **WHEN** the user presses `Esc`
- **THEN** any unsaved changes SHALL be discarded and the screen SHALL return to the previous screen

#### Scenario: Reset with D
- **WHEN** the user presses `D`
- **THEN** both fields SHALL be reset to default values in the UI (changes are not saved until the user presses `S`)

### Requirement: Keybinding hints
The Config screen SHALL display keybinding hints for available actions.

#### Scenario: Hints displayed
- **WHEN** the Config screen is visible
- **THEN** it SHALL show hints for `[S] Save`, `[Esc] Cancel`, `[D] Defaults`, and `[Tab] Switch field`
