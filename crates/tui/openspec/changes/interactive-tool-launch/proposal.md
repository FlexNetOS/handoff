## Why

Users want to launch their AI coding tool (e.g., Claude Code) interactively from within the TUI to work on changes hands-on, without the TUI capturing output or managing the process. Currently the TUI only supports non-interactive batch execution via `--print` mode.

## What Changes

- Add a new `interactive_command` field to `TuiConfig` with default value `"claude"`
- Add keybinding `I` in the change list view to launch the configured interactive command
- Suspend the TUI (leave alternate screen, disable raw mode), run the command, and restore the TUI when the process exits
- Add the `interactive_command` field to the Config screen for editing

## Capabilities

### New Capabilities
- `interactive-tool-launch`: Configurable interactive tool launch from the TUI with terminal suspension and restoration

### Modified Capabilities
- `tui-configuration`: Add `interactive_command` field with default value
- `config-screen`: Add `interactive_command` field to the config editing UI

## Impact

- `config.rs`: New `interactive_command` field on `TuiConfig`, default `"claude"`, serde support
- `app.rs`: Handle `I` keybinding in change list, new method to signal interactive launch
- `main.rs`: Suspend TUI, spawn interactive process (similar to `edit_in_external_editor`), restore TUI
- `ui.rs`: Show `[I] Interactive` in keybinding hints, render `interactive_command` field in Config screen
