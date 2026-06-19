## Context

The artifact view in `ui.rs` (`draw_artifact_view`) currently renders Markdown files as plain text. The content is converted directly into Ratatui `Line` objects using `content.lines().map(Line::from)` – without any parsing or styling.

Ratatui 0.29 already supports rich text via `Span` objects with `Style` (colors, bold, italic, etc.). The only missing piece is the Markdown-to-Spans conversion.

## Goals / Non-Goals

**Goals:**
- Display Markdown content with visual formatting (headers, bold, italic, lists, code, tables)
- Syntax highlighting in code blocks with language support
- Preserve existing scroll and navigation functionality

**Non-Goals:**
- Interactive Markdown elements (clickable links, collapsible sections)
- Editing Markdown in the TUI
- Writing a custom Markdown parser

## Decisions

### tui-markdown as rendering library

**Decision:** Use `tui-markdown` with the `syntect` feature.

**Alternatives considered:**
- **ratskin** (termimad wrapper): Less community adoption, no syntax highlighting, indirect path via termimad
- **Build from scratch** (pulldown-cmark + syntect directly): Significantly more effort, Markdown AST rendering is non-trivial with many edge cases
- **pulldown-cmark without syntect**: No code highlighting

**Rationale:** tui-markdown is maintained by the Ratatui core maintainer (Josh McKinney), has the broadest feature coverage, offers an optional syntect feature, and the API (`from_str() → Text`) fits directly into the existing architecture.

### Minimal change to the rendering pipeline

**Decision:** Only modify `draw_artifact_view` in `ui.rs`. The `Text` generation switches from manual line conversion to `tui_markdown::from_str()`.

**Rationale:** The existing architecture (Screen enum, scroll mechanism, navigation) remains unchanged. The change is limited to the presentation layer.

## Risks / Trade-offs

- **[Compatibility]** tui-markdown uses `ratatui-core ^0.1.0`, which must be compatible with Ratatui 0.29 → Verify during `cargo add`, adjust Ratatui version if needed
- **[Experimental Label]** tui-markdown is marked as "Proof of Concept", API may change → Expected for a v0.x crate; pinning to a specific version minimizes risk
- **[Binary Size]** syntect bundles syntax definitions, which increases binary size → Acceptable trade-off for the functionality provided
- **[Scroll Behavior]** Rendered Markdown lines may differ from raw text lines (e.g., tables, list indentation) → Scroll position logic may need adjustment
