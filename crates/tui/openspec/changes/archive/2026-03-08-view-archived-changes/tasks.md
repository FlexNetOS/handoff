## 1. Data Layer

- [x] 1.1 Add `list_archived_changes()` function to `data.rs` that reads `openspec/changes/archive/` directory entries, parses task progress from each `tasks.md`, and returns a `Vec<ChangeEntry>` sorted by date descending then name ascending
- [x] 1.2 Add `get_archived_change_status()` function to `data.rs` that builds a `ChangeStatusOutput` by checking file existence (proposal.md, design.md, tasks.md, specs/) instead of calling `openspec status`
- [x] 1.3 Add unit tests for `list_archived_changes()` covering: directory with changes, empty directory, non-existent directory, and correct sort order
- [x] 1.4 Add unit tests for `get_archived_change_status()` covering: all artifacts present, some missing, specs with subdirectories

## 2. App State

- [x] 2.1 Add `ChangeTab` enum (`Active`, `Archived`) and add `tab` field to `Screen::ChangeList`
- [x] 2.2 Add `is_archived` field to `Screen::ArtifactMenu`
- [x] 2.3 Implement tab switching in `handle_change_list_input` for Left/Right/h/l keys — reload changes from appropriate data source and reset selection to 0
- [x] 2.4 Update `enter_artifact_menu` to use `get_archived_change_status()` when on the Archived tab and set `is_archived` flag
- [x] 2.5 Update `find_change_dir` to resolve to `openspec/changes/archive/<name>/` when `is_archived` is true
- [x] 2.6 Guard the `R` key handler in `handle_artifact_menu_input` to do nothing when `is_archived` is true
- [x] 2.7 Add unit tests for tab switching, archived artifact menu entry, and runner deactivation on archived changes

## 3. UI Rendering

- [x] 3.1 Update `draw_change_list` to accept the current tab and render the title as `OpenSpec TUI [Active | Archived]` with the active tab highlighted
- [x] 3.2 Add unit tests for title rendering on both tabs

## 4. Main Loop

- [x] 4.1 Update `App::new()` to initialize `ChangeList` with `tab: ChangeTab::Active`
- [x] 4.2 Verify Left/Right key events reach `handle_change_list_input` (already handled by existing dispatch)
