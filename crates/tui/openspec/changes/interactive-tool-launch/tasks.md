## 1. Config Layer

- [x] 1.1 Add `interactive_command` field to `TuiConfig` with default `"claude"` and serde support
- [x] 1.2 Add `build_interactive_command()` method that splits the command string on whitespace and returns `(binary, args)`
- [x] 1.3 Add unit tests for `interactive_command` default, serialization, deserialization, and `build_interactive_command()`

## 2. App Layer

- [x] 2.1 Add `InteractiveLaunch` variant to the signal mechanism (or a boolean flag) so `main.rs` knows to suspend the TUI
- [x] 2.2 Handle `I` keybinding in `handle_list_input` — signal interactive launch only on Active tab and when no implementation is running
- [x] 2.3 Add unit tests for `I` key handling: signal on Active tab, no signal on Archived tab, no signal during running implementation

## 3. Main Loop

- [x] 3.1 Add `launch_interactive_tool()` function in `main.rs` that suspends the TUI, runs the command via `.status()`, and restores the TUI
- [x] 3.2 Wire the signal from `app.rs` into `main.rs` event loop to call `launch_interactive_tool()` and reload data afterward

## 4. Config Screen

- [x] 4.1 Add `interactive_command` field to the `Screen::Config` variant and wire it through `push_config_screen`, save, and reset-defaults
- [x] 4.2 Add `InteractiveCommand` variant to `ConfigField` enum and update Tab cycling to include it
- [x] 4.3 Handle Enter on `InteractiveCommand` field for inline editing (same as Command field)
- [x] 4.4 Add unit tests for Config screen: Tab cycling includes InteractiveCommand, Enter activates inline edit, save persists the field, reset defaults restores `"claude"`

## 5. UI Rendering

- [x] 5.1 Render `interactive_command` field in the Config screen UI
- [x] 5.2 Add `[I] Interactive` to keybinding hints in the change list view (Active tab only)
- [x] 5.3 Add unit tests for UI rendering: interactive_command field visible in Config screen, `[I]` hint visible in change list
