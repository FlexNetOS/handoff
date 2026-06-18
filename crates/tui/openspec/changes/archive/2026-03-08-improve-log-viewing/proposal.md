## Why

The implementation log (`implementation.log`) is rendered through the Markdown parser (`tui_markdown::from_str()`), which collapses single newlines into spaces and misinterprets separator lines (`══════`, `──────`). This makes the log hard to read. Additionally, after starting the implementation runner with `R`, the user stays on the artifact menu with no immediate way to see the log output. There is also no shortcut to quickly jump to the log.

## What Changes

- Render `.log` files as plain text instead of Markdown, preserving all newlines and formatting
- Automatically open the log view after starting the implementation runner with `R`
- Add `L` shortcut in the artifact menu to directly open the implementation log

## Capabilities

### New Capabilities

_(none)_

### Modified Capabilities

- `artifact-content-view`: Add plain-text rendering mode for log files (skip Markdown parsing, preserve newlines verbatim)
- `artifact-menu-view`: Add `L` keyboard shortcut to open implementation log directly; auto-navigate to log view after runner start

## Impact

- `src/ui.rs`: New plain-text rendering path in `draw_artifact_view` (or separate function)
- `src/app.rs`: `L` shortcut handler in artifact menu input; auto-navigate to log view after `R` press
