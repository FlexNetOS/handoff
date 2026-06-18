## Why

The openspec change directory contains the proposal, design, specs, and tasks for the current work-in-progress. There is no quick way to browse these artifacts while working. A terminal-based viewer lets developers reference their active change context without leaving the terminal.

## What Changes

- Add a ratatui-based TUI application that reads openspec data via the `openspec` CLI
- Screen 1: List active changes (from `openspec list --json`), selectable with keyboard
- Screen 2: Show available artifacts for a selected change (Proposal, Design, Tasks, Specs), greyed out when not yet created
- Screen 3: Scrollable plain-text viewer displaying the raw markdown content of the selected artifact
- Navigation: arrow keys / j/k to move, Enter to select, Esc to go back, q to quit

## Capabilities

### New Capabilities
- `change-list-view`: TUI screen listing active openspec changes with keyboard navigation
- `artifact-menu-view`: TUI screen showing available artifacts for a change with availability indicators
- `artifact-content-view`: Scrollable plain-text viewer for artifact markdown files

### Modified Capabilities

## Impact

- New dependencies: `ratatui`, `crossterm` (terminal backend)
- Uses `openspec` CLI (must be on PATH) for change listing and artifact status
- Reads artifact files directly from disk for content display
- No impact on existing openspec data or workflows
