## Why

The TUI loads data once when entering a screen and never updates it. If external changes occur (e.g., tasks completed by another process, files edited in an editor, new changes created via CLI), the user must leave and re-enter a screen to see updates. A manual refresh mechanism lets users reload the current view's data without losing their navigation context.

## What Changes

- Add `r` key as a global refresh trigger across all applicable screens
- Each screen reloads its data from the underlying sources (CLI, filesystem)
- Selection index is preserved after refresh (clamped to new list length if needed)
- Config screen is excluded from refresh (it is edited directly in the TUI)

## Capabilities

### New Capabilities
- `view-refresh`: Manual refresh of the current screen's data via the `r` key, covering ChangeList, ArtifactMenu, ArtifactView, DependencyView, DependencyGraph, and RunAllSelection screens

### Modified Capabilities

## Impact

- `src/app.rs`: New refresh methods on `App`, `r` key handling in input handlers
- `src/main.rs`: Possibly a global key check before screen-specific dispatch
- `src/data.rs`: No changes expected — existing data functions are already suitable for re-calling
