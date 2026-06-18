## Context

The TUI displays active changes using `openspec list --json` and `openspec status --change <name> --json`. Archived changes live in `openspec/changes/archive/<date>-<name>/` with the same file structure (proposal.md, design.md, tasks.md, specs/) but have no CLI support for listing or status queries.

The current `ChangeList` screen has no concept of tabs or multiple data sources. The `ArtifactMenu` screen relies on `openspec status` to determine artifact availability, and the implementation runner is always available via the `R` key.

## Goals / Non-Goals

**Goals:**
- Browse archived changes in the TUI with tab-based navigation
- View all artifacts of archived changes (proposal, design, specs, tasks, implementation log)
- Show task progress for archived changes
- Disable the implementation runner for archived changes
- Sort archived changes by date descending, then name ascending

**Non-Goals:**
- Re-running or modifying archived changes
- Searching or filtering changes
- Unarchiving changes from the TUI

## Decisions

### 1. Tab state as enum on ChangeList screen

Add a `ChangeTab` enum (`Active`, `Archived`) to the `ChangeList` screen variant. When the tab changes, the `changes` vector is repopulated from the appropriate data source.

**Alternative considered**: Separate screen variants for active and archived lists. Rejected because the screens are structurally identical — only the data source and a few behaviors differ.

### 2. Direct filesystem reading for archived changes

Create a `list_archived_changes()` function in `data.rs` that reads `openspec/changes/archive/` directory entries. Each subdirectory becomes a `ChangeEntry` with task progress parsed from `tasks.md` via the existing `parse_task_progress()` function.

**Alternative considered**: Extending the openspec CLI to support archive listing. Rejected because the CLI doesn't support it and this is a TUI-only concern.

### 3. Filesystem-based artifact detection for archived changes

Instead of calling `openspec status`, create a `get_archived_change_status()` function that checks which artifact files exist in the archive directory. All existing files are treated as "done". This returns a `ChangeStatusOutput` compatible with the existing `build_artifact_menu_items()` function.

**Alternative considered**: A separate artifact menu builder for archived changes. Rejected because the existing `build_artifact_menu_items()` already works with `ChangeStatusOutput` — we just need a different way to produce that struct.

### 4. Sorting: date prefix extraction

Archive directory names follow the pattern `YYYY-MM-DD-<name>`. Sort by extracting the first 10 characters as the date component:
- Primary: date descending (newest first)
- Secondary: remainder after date prefix ascending (alphabetical)

### 5. Archived flag propagated to ArtifactMenu

Add an `is_archived` boolean to the `ArtifactMenu` screen variant. This is set when entering the artifact menu from an archived change. The `R` key handler checks this flag and does nothing when true.

### 6. Tab switching via Left/Right and h/l keys

On the `ChangeList` screen, Left/Right arrow keys and `h`/`l` toggle between Active and Archived tabs. This is handled in `handle_change_list_input`. The selection index resets to 0 on tab switch.

### 7. Archive path resolution in find_change_dir

`find_change_dir` needs to resolve to `openspec/changes/archive/<name>/` for archived changes. Pass the `is_archived` flag (derived from the current tab) to determine the correct base path.

## Risks / Trade-offs

- **Archive directory not existing**: If `openspec/changes/archive/` doesn't exist, `list_archived_changes()` returns an empty list. No error shown — same behavior as active changes with no results.
- **Inconsistent archive naming**: If archive directories don't follow the `YYYY-MM-DD-<name>` pattern, the sort will still work (treating the full name as the sort key) but date grouping won't be meaningful. Acceptable since the archive command controls the naming.
- **No live refresh**: Archived changes are loaded once when switching to the tab. If changes are archived while the TUI is open, the user must switch tabs to reload. Same limitation as active changes.
