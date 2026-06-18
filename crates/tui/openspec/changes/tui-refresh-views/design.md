## Context

The TUI loads screen data once on navigation and does not update it afterwards. Users working with external tools (editor, CLI) need a way to see fresh data without navigating away and back. The existing architecture uses a `Screen` enum with mutable fields — data can be replaced in-place by re-running the same data functions used during screen initialization.

## Goals / Non-Goals

**Goals:**
- Allow users to press `r` to reload data on any non-Config screen
- Preserve selection index (clamped to new data length) after refresh
- Keep the approach minimal — reuse existing data functions, no new infrastructure

**Non-Goals:**
- Auto-refresh / periodic polling
- Visual feedback (flash, spinner) during refresh
- Refreshing the Config screen (edited directly in TUI)

## Decisions

### 1. Single `refresh_screen()` method on App

Add one public method `App::refresh_screen()` that matches on `self.screen` and reloads data per variant. This keeps the refresh logic centralized rather than scattered across individual input handlers.

**Alternative considered:** Adding `r` handling inside each `handle_*_input()` method. Rejected because it duplicates the pattern and makes it easy to forget a screen.

### 2. Handle `r` as a global key in `run_app()` event loop

Place the `r` check in `main.rs` alongside the existing global keys (`q`, `S`), before the screen-specific `match`. This mirrors the existing pattern and ensures `r` works everywhere without per-handler code.

**Alternative considered:** Handling `r` inside each `handle_*_input()`. Rejected — same reasoning as above.

### 3. Per-screen refresh logic

| Screen | Refresh action |
|--------|---------------|
| ChangeList (Active) | Re-call `data::list_changes()`, rebuild `change_deps` |
| ChangeList (Archived) | Re-call `data::list_archived_changes()` |
| ArtifactMenu | Re-call `data::get_change_status()` or `get_archived_change_status()`, rebuild menu items |
| ArtifactView | Re-call `data::read_artifact_content()` on same file path |
| DependencyView | Re-call `data::read_dependencies()` |
| DependencyGraph | Rebuild from current change_deps (requires reloading changes + deps first) |
| RunAllSelection | Rebuild entries from fresh change list |
| Config | No-op |
| DependencyAdd | Rebuild available changes list |

ArtifactView needs access to the file path, which is not currently stored in the `Screen::ArtifactView` variant. The `title` field contains display text, not the path. We need to add a `file_path` field to `ArtifactView` to support refresh.

### 4. Selection clamping

After reloading, clamp `selected` to `new_length.saturating_sub(1)`. This prevents index-out-of-bounds if items were removed.

## Risks / Trade-offs

- **[CLI latency]** Refreshing ChangeList calls `openspec list --json` which spawns a subprocess. This is the same cost as initial load and tab switching, which is already accepted. → No mitigation needed.
- **[ArtifactView schema change]** Adding `file_path` to `ArtifactView` requires updating all places that construct this variant. → Low risk, compiler will catch all sites.
