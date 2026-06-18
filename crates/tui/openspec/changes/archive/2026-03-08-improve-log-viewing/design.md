## Context

The TUI currently renders all artifact content through `tui_markdown::from_str()` in `draw_artifact_view()`. This works well for Markdown files (proposal.md, design.md, etc.) but breaks for the implementation log, which is plain text with single newlines, separator lines (`══════`, `──────`), and mixed formatting that Markdown parsers collapse or misinterpret.

After starting the implementation runner with `R`, the user remains on the artifact menu with no immediate visibility into the log. The log only appears as a menu entry after re-opening the artifact menu.

## Goals / Non-Goals

**Goals:**
- Render log files as plain text, preserving all newlines and whitespace verbatim
- Auto-navigate to the log view when the runner starts
- Provide a direct `L` shortcut in the artifact menu to open the log

**Non-Goals:**
- Live-updating log view (auto-refresh while runner is active)
- Supporting other plain-text file types beyond `.log`

## Decisions

### 1. Add a `is_plain_text` flag to `Screen::ArtifactView`

**Decision:** Add a boolean `is_plain_text` field to the `ArtifactView` variant rather than creating a separate screen variant.

**Rationale:** A separate `PlainTextView` screen would duplicate scrolling logic and input handling. A flag keeps the code DRY — the only difference is the rendering path in `draw_artifact_view()`.

**Alternative considered:** Detecting plain text by file extension in the UI layer. Rejected because the UI layer shouldn't know about file paths; the decision belongs in `app.rs` when constructing the screen.

### 2. Plain-text rendering via `Paragraph::new(content)` without Markdown parsing

**Decision:** When `is_plain_text` is true, skip `tui_markdown::from_str()` and pass the raw content string directly to `Paragraph::new()`. This preserves all newlines and whitespace.

**Rationale:** Ratatui's `Paragraph` widget handles plain text naturally. No additional crate or custom parser needed.

### 3. Detect log files by extension in `app.rs`

**Decision:** When opening an artifact via Enter or the `L` shortcut, check if the file path ends with `.log` and set `is_plain_text = true`.

**Rationale:** Simple, explicit, and keeps the detection logic in the application layer where screen construction happens.

### 4. Auto-navigate to log view after pressing `R`

**Decision:** After starting the runner with `R`, immediately push the current ArtifactMenu onto the screen stack and navigate to `ArtifactView` showing the log file (which may be empty initially).

**Rationale:** The user pressed `R` because they want to see what happens. Showing the log immediately is the expected UX. The log file is created by the runner at start, so it exists even if empty.

### 5. `L` shortcut opens log from artifact menu

**Decision:** Add `KeyCode::Char('L')` handler in `handle_artifact_menu_input()` that opens the implementation log directly, regardless of whether it appears in the menu items.

**Rationale:** Quick access without scrolling through the menu. Uses uppercase `L` consistent with the `R` (Run) convention.

## Risks / Trade-offs

- **[Empty log on auto-open]** → The log file may be empty or near-empty right after runner start. This is acceptable — the user can press Esc and re-open later. The status bar still shows live progress.
- **[is_plain_text flag sprawl]** → Adding a flag to `ArtifactView` adds a small amount of complexity. Mitigation: it's a single boolean with a clear purpose.
