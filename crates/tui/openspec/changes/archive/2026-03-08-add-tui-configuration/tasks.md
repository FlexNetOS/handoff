## 1. Configuration Data Model

- [x] 1.1 Add `serde_yaml` dependency to Cargo.toml
- [x] 1.2 Create `TuiConfig` struct with `command` and `prompt` fields, with `Default` implementation using current hardcoded values
- [x] 1.3 Implement `TuiConfig::load()` to read from `openspec/tui-config.yaml` with fallback to defaults for missing fields
- [x] 1.4 Implement `TuiConfig::save()` to write to `openspec/tui-config.yaml`
- [x] 1.5 Add tests for config loading (missing file, partial fields, full file) and saving

## 2. Placeholder Substitution

- [x] 2.1 Implement `TuiConfig::render_prompt(name: &str)` that replaces `{name}` in the prompt template
- [x] 2.2 Implement `TuiConfig::build_command(prompt: &str)` that replaces `{prompt}` in the command template, splits on whitespace, and returns binary + args
- [x] 2.3 Add tests for placeholder substitution in both prompt and command

## 3. Runner Integration

- [x] 3.1 Add `TuiConfig` parameter to `start_implementation()` and the runner loop
- [x] 3.2 Replace hardcoded `build_prompt()` with `config.render_prompt(change_name)`
- [x] 3.3 Replace `data::claude_command()` invocation with `config.build_command(prompt)` to construct the process
- [x] 3.4 Remove `claude_command()` from `data.rs`
- [x] 3.5 Add tests for runner using config-driven command and prompt

## 4. Config Screen UI

- [x] 4.1 Add `Screen::Config` variant with fields for command text, prompt text, cursor position, and focused field
- [x] 4.2 Implement `draw_config_screen()` in `ui.rs` with command input field, prompt preview, and keybinding hints
- [x] 4.3 Implement `handle_config_input()` in `app.rs` with inline text editing for command field (character input, cursor movement, backspace, delete, home, end)
- [x] 4.4 Implement Tab to switch focus between command and prompt fields
- [x] 4.5 Implement Enter on prompt field to open `$EDITOR` (fallback `vi`) with temp file, read back on exit
- [x] 4.6 Implement `S` to save config and return to previous screen
- [x] 4.7 Implement `Esc` to discard changes and return to previous screen
- [x] 4.8 Implement `D` to reset fields to default values in the UI

## 5. Config Screen Access

- [x] 5.1 Add `C` keybinding in `handle_change_list_input()` to push Config screen
- [x] 5.2 Add `C` keybinding in `handle_artifact_menu_input()` to push Config screen
- [x] 5.3 Add `C` keybinding in `handle_artifact_view_input()` to push Config screen
- [x] 5.4 Load config at app startup and store in `App` struct

## 6. Cleanup

- [x] 6.1 Remove the hardcoded `build_prompt()` function from `runner.rs`
- [x] 6.2 Remove `claude_command()` from `data.rs` (keep `openspec_command()`)
- [x] 6.3 Update keybinding hints in existing screens to include `[C] Config`
