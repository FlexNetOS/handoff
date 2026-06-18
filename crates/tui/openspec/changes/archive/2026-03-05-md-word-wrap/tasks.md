## 1. Core Implementation

- [x] 1.1 Add `use ratatui::widgets::Wrap;` import to `src/ui.rs`
- [x] 1.2 Add `.wrap(Wrap { trim: false })` to the `Paragraph` widget in `draw_artifact_view`

## 2. Testing

- [x] 2.1 Add test that `draw_artifact_view` renders wrapped lines for long content
- [x] 2.2 Add test that short lines remain unchanged after wrapping
- [x] 2.3 Add test that leading whitespace is preserved on wrapped content
