## ADDED Requirements

### Requirement: Config screen has navigation and edit modes
The config screen SHALL have two input modes: navigation mode and edit mode. Navigation mode SHALL be the default when entering the config screen.

#### Scenario: Config screen opens in navigation mode
- **WHEN** the user opens the config screen
- **THEN** the screen is in navigation mode with the Command field focused

#### Scenario: No text input in navigation mode
- **WHEN** the user types a character in navigation mode
- **THEN** the character is NOT inserted into any field

### Requirement: Navigation mode supports field switching
The user SHALL be able to switch between fields using Tab and BackTab in navigation mode.

#### Scenario: Tab switches from Command to Prompt
- **WHEN** the user presses Tab in navigation mode with Command focused
- **THEN** the focused field changes to Prompt

#### Scenario: Tab switches from Prompt to Command
- **WHEN** the user presses Tab in navigation mode with Prompt focused
- **THEN** the focused field changes to Command

### Requirement: Enter activates editing for the focused field
Pressing Enter in navigation mode SHALL activate the appropriate editing mechanism for the focused field.

#### Scenario: Enter on Command field activates inline edit mode
- **WHEN** the user presses Enter in navigation mode with Command focused
- **THEN** the screen enters edit mode with the cursor at the end of the command text

#### Scenario: Enter on Prompt field opens external editor
- **WHEN** the user presses Enter in navigation mode with Prompt focused
- **THEN** the system opens `$EDITOR` with the prompt text

### Requirement: Escape returns from edit mode to navigation mode
Pressing Escape in edit mode SHALL return to navigation mode without discarding edits made in the current editing session.

#### Scenario: Escape in edit mode returns to navigation
- **WHEN** the user presses Escape while editing the Command field
- **THEN** the screen returns to navigation mode and the edited text is preserved

#### Scenario: Enter in edit mode also returns to navigation
- **WHEN** the user presses Enter while editing the Command field
- **THEN** the screen returns to navigation mode and the edited text is preserved

### Requirement: Escape in navigation mode exits config screen
Pressing Escape in navigation mode SHALL discard unsaved changes and return to the previous screen.

#### Scenario: Escape exits config screen
- **WHEN** the user presses Escape in navigation mode
- **THEN** the config screen closes and the previous screen is restored

### Requirement: Save shortcut works in navigation mode
The user SHALL be able to save the config by pressing `S` in navigation mode.

#### Scenario: S saves config and exits
- **WHEN** the user presses `S` in navigation mode
- **THEN** the config is saved to `openspec/tui-config.yaml` and the previous screen is restored

### Requirement: Reset defaults shortcut works in navigation mode
The user SHALL be able to reset config to defaults by pressing `D` in navigation mode.

#### Scenario: D resets fields to defaults
- **WHEN** the user presses `D` in navigation mode
- **THEN** both command and prompt fields are reset to their default values

### Requirement: Command field supports inline text editing in edit mode
When in edit mode on the Command field, the user SHALL be able to edit text inline with standard editing keys.

#### Scenario: Character insertion
- **WHEN** the user types a character in edit mode
- **THEN** the character is inserted at the cursor position

#### Scenario: Backspace deletes character before cursor
- **WHEN** the user presses Backspace in edit mode with cursor after position 0
- **THEN** the character before the cursor is deleted and the cursor moves left

#### Scenario: Cursor movement with arrow keys
- **WHEN** the user presses Left or Right arrow in edit mode
- **THEN** the cursor moves one position in the corresponding direction

### Requirement: Visual distinction between navigation and edit modes
The config screen SHALL visually indicate whether the user is in navigation or edit mode.

#### Scenario: Navigation mode shows no cursor in command field
- **WHEN** the screen is in navigation mode
- **THEN** the Command field does NOT display a block cursor

#### Scenario: Edit mode shows cursor in command field
- **WHEN** the screen is in edit mode on the Command field
- **THEN** the Command field displays a visible block cursor at the cursor position

### Requirement: Keybinding hints reflect current mode
The keybinding hints at the bottom of the config screen SHALL reflect the available actions for the current mode.

#### Scenario: Navigation mode hints
- **WHEN** the screen is in navigation mode
- **THEN** hints show `[Enter] Edit`, `[Tab] Switch field`, `[S] Save`, `[D] Reset defaults`, `[Esc] Cancel`

#### Scenario: Edit mode hints
- **WHEN** the screen is in edit mode
- **THEN** hints show `[Esc] Done editing`
