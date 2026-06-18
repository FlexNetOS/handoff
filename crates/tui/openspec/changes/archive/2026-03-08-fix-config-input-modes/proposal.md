## Why

The config screen currently has no separation between navigation and text editing. All keypresses in the Command field are captured as text input, making shortcuts like `S` (save) and `D` (reset defaults) unreachable while editing. The screen also starts with the Command field active, so users must first Tab to the Prompt field before any shortcut works.

## What Changes

- Introduce a **navigation mode** as the default state on the config screen, where shortcuts (`S`, `D`, `Tab`, `Esc`, `Enter`) work as expected
- Pressing `Enter` on the Command field activates **edit mode** for inline text editing
- Pressing `Enter` on the Prompt field opens `$EDITOR` (existing behavior, now explicit)
- Pressing `Esc` in edit mode returns to navigation mode (instead of leaving the config screen)
- Visual distinction between focused-but-not-editing and actively-editing states
- Delete the stale `openspec/tui-config.yaml` test data file so defaults load correctly

## Capabilities

### New Capabilities
- `config-screen`: Specification for the TUI configuration screen including navigation/edit mode behavior, keybindings, and visual states

### Modified Capabilities

## Impact

- `src/app.rs`: `ConfigField` enum needs an editing flag or new enum, `handle_config_input` logic restructured around two modes
- `src/ui.rs`: `draw_config_screen` needs visual differentiation between navigation and edit states
- `openspec/tui-config.yaml`: Remove stale test data
