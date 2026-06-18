## Why

The TUI currently only shows active changes from `openspec list --json`. Archived changes stored in `openspec/changes/archive/` are invisible in the TUI. Users want to browse completed work — reviewing past proposals, designs, specs, and tasks — without leaving the TUI or navigating the filesystem manually.

## What Changes

- Add a tab-based view to the ChangeList screen: **Active** (default) and **Archived**
- The active tab indicator is shown in the title bar: `OpenSpec TUI [Active | Archived]`
- Users switch tabs with Left/Right arrow keys (or `h`/`l`) on the ChangeList screen
- Archived changes are loaded by reading `openspec/changes/archive/` directory entries directly (no CLI support exists)
- Archived changes display task progress (completed/total) parsed from their `tasks.md`
- Archived changes are sorted: date descending (newest first), then name ascending (alphabetical within same date)
- The artifact menu for archived changes works identically to active changes (view proposal, design, specs, tasks, implementation log)
- The implementation runner (`R` key) is completely disabled for archived changes

## Capabilities

### New Capabilities
- `archived-change-browsing`: Browsing archived changes via tab-based navigation in the ChangeList screen, including filesystem-based data loading and archive-aware artifact menu

### Modified Capabilities
- `change-list-view`: Add tab switching between active and archived change lists
- `artifact-menu-view`: Disable implementation runner for archived changes; support filesystem-based artifact detection (no `openspec status` for archived changes)

## Impact

- `src/data.rs`: New functions to list archived changes from filesystem and build artifact status without CLI
- `src/app.rs`: `ChangeList` screen gains a `tab` field; `ArtifactMenu` gains an `is_archived` flag; `R` key handler checks archived status; `find_change_dir` resolves archive paths
- `src/ui.rs`: Title bar shows tab indicator; tab styling for active/inactive
- `src/main.rs`: Left/Right key handling for tab switching on ChangeList screen
