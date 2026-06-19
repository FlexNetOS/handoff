## 1. Plain-Text Rendering

- [x] 1.1 Add `is_plain_text: bool` field to `Screen::ArtifactView` in `src/app.rs`
- [x] 1.2 Update `draw_artifact_view()` in `src/ui.rs` to accept `is_plain_text` parameter and skip `tui_markdown::from_str()` when true, passing raw content to `Paragraph::new()` instead
- [x] 1.3 Update all call sites that construct `Screen::ArtifactView` to set `is_plain_text` based on file extension (`.log` → true, otherwise false)
- [x] 1.4 Add tests: plain-text rendering preserves single newlines, separator lines render verbatim, non-log files still use Markdown

## 2. Log Shortcut

- [x] 2.1 Add `KeyCode::Char('L')` handler in `handle_artifact_menu_input()` that opens `implementation.log` from the change directory as plain text, if the file exists
- [x] 2.2 Add tests: `L` opens log when file exists, `L` is no-op when file does not exist, `L` works for archived changes

## 3. Auto-Navigate After Runner Start

- [x] 3.1 After `runner::start_implementation()` in the `R` handler, push current screen to stack and navigate to `ArtifactView` with the log content and `is_plain_text: true`
- [x] 3.2 Add tests: pressing `R` starts runner AND navigates to log view, Esc returns to artifact menu
