## Context

This is a greenfield Rust project (edition 2024) with no application code yet. The openspec CLI is already available and provides JSON output for listing changes and checking artifact status. The TUI needs to shell out to `openspec` for metadata and read files directly for content.

## Goals / Non-Goals

**Goals:**
- Provide a keyboard-navigable TUI to browse active openspec changes and their artifacts
- Use the openspec CLI as the data source for change listing and artifact availability
- Display artifact content as plain text (raw markdown)
- Grey out unavailable artifacts so the user knows what exists

**Non-Goals:**
- Markdown rendering (bold, headers, colors) — plain text is sufficient for v1
- Archive browsing — only active changes
- Editing or creating changes from the TUI
- Watching for file changes / live reload

## Decisions

### TUI framework: ratatui + crossterm
ratatui is the standard Rust TUI library with an active ecosystem. crossterm provides the cross-platform terminal backend.

### App architecture: Enum-based screen state machine
Three screens modeled as an enum (`ChangeList`, `ArtifactMenu`, `ArtifactView`). Each screen holds its own state (selected index, scroll offset, loaded data). Transitions happen on Enter (push deeper) and Esc (pop back). This is simple and avoids framework overhead.

```
ChangeList ──Enter──▶ ArtifactMenu ──Enter──▶ ArtifactView
     ◀──Esc──              ◀──Esc──
```

### Data loading: openspec CLI via `std::process::Command`
- `openspec list --json` on startup to get active changes
- `openspec status --change <name> --json` when entering a change to get artifact availability
- Direct file reads for artifact content (proposal.md, design.md, tasks.md, specs/*/spec.md)

JSON parsing with `serde` + `serde_json`. This avoids reimplementing openspec's logic and stays in sync with the CLI.

### Artifact menu structure
Fixed list of items: Proposal, Design, Tasks, Specs. Each item maps to an artifact ID from `openspec status`. If status is not `"done"`, the item is rendered in a dimmed style and Enter is a no-op. Specs is a parent item that expands to show individual spec files (discovered by reading the `specs/` subdirectory of the change).

### Navigation
- `↑`/`↓`/`j`/`k`: Move selection
- `Enter`: Select / drill in
- `Esc`: Go back one screen
- `q`: Quit from any screen

## Risks / Trade-offs

- **openspec CLI dependency**: The TUI requires `openspec` on PATH. If not found, it should show a clear error message at startup. → Acceptable since this tool is for openspec users.
- **No live reload**: If artifacts change on disk while the TUI is open, it won't reflect updates. → Acceptable for v1; user can quit and reopen.
- **Shelling out for data**: Adds subprocess overhead on each screen transition. → Negligible for this use case; change lists are small.
