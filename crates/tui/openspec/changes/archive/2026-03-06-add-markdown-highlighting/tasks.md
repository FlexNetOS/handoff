## 1. Dependencies

- [x] 1.1 Add `tui-markdown` with `syntect` feature to `Cargo.toml`
- [x] 1.2 Verify that `tui-markdown` is compatible with `ratatui 0.29` (cargo check)

## 2. Core Implementation

- [x] 2.1 Update `draw_artifact_view` in `ui.rs`: use `tui_markdown::from_str()` instead of manual line conversion
- [x] 2.2 Ensure scroll mechanism works correctly with rendered Markdown text

## 3. Tests

- [x] 3.1 Update existing tests in `ui.rs` (word-wrap tests now work with Markdown-rendered text)
- [x] 3.2 Add test: Markdown headers are rendered as formatted text
- [x] 3.3 Add test: Code blocks are rendered with syntax highlighting
- [x] 3.4 Run all tests successfully (`cargo test`)
