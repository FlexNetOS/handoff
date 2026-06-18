## Context

The TUI displays artifact content (Markdown files) using ratatui's `Paragraph` widget in `draw_artifact_view`. Currently, lines are split only at newline characters and rendered as-is. Lines exceeding the terminal width are truncated at the viewport edge, making prose-heavy Markdown files unreadable.

Ratatui provides a built-in `Wrap` option for the `Paragraph` widget that handles word wrapping at widget boundaries.

## Goals / Non-Goals

**Goals:**
- Enable word wrapping in the artifact content view so all text is visible without horizontal truncation
- Wrap at word boundaries for readability (not mid-character)

**Non-Goals:**
- Markdown rendering (bold, headers, syntax highlighting) – out of scope
- Horizontal scrolling as an alternative to wrapping
- Configurable wrap on/off toggle

## Decisions

### Use ratatui's built-in `Wrap { trim: false }`

**Rationale:** Ratatui's `Paragraph` widget natively supports `.wrap(Wrap { trim: false })`. This is a one-line change that handles word-boundary wrapping correctly. No custom wrapping logic needed.

- `trim: false` preserves leading whitespace, which is important for indented Markdown content (code blocks, nested lists).
- Alternative considered: manual text wrapping before creating `Line` items – rejected because it duplicates built-in functionality and adds complexity.

### Keep existing scroll logic as-is initially

**Rationale:** Ratatui's `Paragraph::scroll((y, x))` scrolls by rendered lines (post-wrap), so the existing scroll-by-one behavior will naturally scroll through wrapped lines. The scroll offset and total line count in the title bar currently reflect source lines, which may differ from rendered lines after wrapping. This is an acceptable trade-off for the initial implementation since the content remains fully scrollable.

## Risks / Trade-offs

- **[Line count display inaccuracy]** → The title bar shows `[line/total]` based on source lines, not rendered lines. After wrapping, the actual number of visible lines may be higher. This is cosmetic and can be addressed in a follow-up if needed.
- **[Indentation preservation]** → Using `trim: false` preserves leading whitespace. This is the correct behavior for Markdown with code blocks but may cause slightly uneven left margins on continuation lines. Acceptable trade-off for content fidelity.
