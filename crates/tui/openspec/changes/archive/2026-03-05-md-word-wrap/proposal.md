## Why

Long lines in Markdown artifact files are truncated at the terminal viewport edge, making content unreadable. Since Markdown files commonly contain prose paragraphs, users cannot read the full content without an external editor. Adding word wrapping will make all content visible within the TUI.

## What Changes

- Enable word wrapping on the `Paragraph` widget used in `draw_artifact_view` so that lines longer than the terminal width break at word boundaries
- Adjust vertical scroll calculation to account for wrapped lines (a single source line may occupy multiple display lines)

## Capabilities

### New Capabilities
- `md-word-wrap`: Word wrapping for artifact content view so long lines break at word boundaries instead of being truncated

### Modified Capabilities
- `artifact-content-view`: Scrolling behavior must account for wrapped lines affecting total visible line count

## Impact

- `src/ui.rs` – `draw_artifact_view` function: add `Wrap` configuration to `Paragraph`
- `src/ui.rs` / `src/app.rs` – scroll logic may need adjustment for wrapped line counts
- No new dependencies required (ratatui's `Wrap` is built-in)
