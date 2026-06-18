## Why

Markdown files (proposal.md, design.md, tasks.md, specs) are currently displayed as plain text. Headers, bold, code blocks, and other formatting are not visually distinguishable, which significantly limits readability. Syntax highlighting makes the content much easier to parse and navigate.

## What Changes

- Markdown content is displayed with formatted rendering (headers, bold, italic, code blocks, lists, tables, etc.)
- Code blocks receive syntax highlighting with language support
- New dependency: `tui-markdown` (with `syntect` feature) for Markdown-to-Ratatui conversion
- The existing plain text display in `draw_artifact_view` is replaced with rendered Markdown

## Capabilities

### New Capabilities
- `markdown-rendering`: Conversion of Markdown content into formatted Ratatui widgets with syntax highlighting for code blocks

### Modified Capabilities
- `artifact-content-view`: The artifact display switches from plain text to rendered Markdown

## Impact

- **Code**: `ui.rs` (`draw_artifact_view` function) is modified
- **Dependencies**: `tui-markdown` (with `syntect` feature) is added to `Cargo.toml`
- **Compatibility**: `tui-markdown` uses `ratatui-core ^0.1.0` – must be compatible with our `ratatui 0.29`
