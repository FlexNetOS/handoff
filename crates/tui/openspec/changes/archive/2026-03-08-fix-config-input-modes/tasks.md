## 1. Add editing flag to Screen::Config

- [x] 1.1 Add `editing: bool` field to the `Screen::Config` variant in `src/app.rs`, default to `false` when opening config
- [x] 1.2 Update `draw_config_screen` in `src/ui.rs` to accept the `editing` parameter and only show the block cursor when `editing == true && focused_field == Command`

## 2. Restructure key handling for navigation mode

- [x] 2.1 Refactor `handle_config_input` so that when `editing == false`, `S` saves, `D` resets defaults, `Esc` exits config, `Enter` activates edit mode (or opens `$EDITOR` for Prompt), and `Tab`/`BackTab` switch fields
- [x] 2.2 Ensure character keys are ignored in navigation mode (no text insertion)

## 3. Restructure key handling for edit mode

- [x] 3.1 When `editing == true`, route character keys, Backspace, Delete, Left, Right, Home, End to inline command editing (existing logic)
- [x] 3.2 `Esc` in edit mode sets `editing = false` (returns to navigation) without discarding edits
- [x] 3.3 `Enter` in edit mode sets `editing = false` (returns to navigation) without discarding edits

## 4. Update keybinding hints

- [x] 4.1 Show navigation-mode hints (`[Enter] Edit`, `[Tab] Switch field`, `[S] Save`, `[D] Reset defaults`, `[Esc] Cancel`) when `editing == false`
- [x] 4.2 Show edit-mode hints (`[Esc] Done editing`) when `editing == true`
- [x] 4.3 Keep the `{prompt} missing` warning visible in both modes

## 5. Cleanup and tests

- [x] 5.1 Delete stale `openspec/tui-config.yaml` test data file
- [x] 5.2 Add tests for navigation mode: S saves, D resets, Esc exits, Enter activates edit mode, character keys are ignored
- [x] 5.3 Add tests for edit mode: character insertion works, Esc returns to navigation, Enter returns to navigation
- [x] 5.4 Add tests for visual state: cursor only visible in edit mode, correct hints per mode
