## Why

The implementation runner currently writes its log to a temporary file in `/tmp/`, which is lost after reboot and has no connection to the change it belongs to. Users must manually find and open the log file outside the TUI. Moving the log into the change directory makes it persistent, discoverable, and viewable as a regular artifact.

## What Changes

- Move log file from `std::env::temp_dir()` to `openspec/changes/<name>/implementation.log`
- Write clear header markers when a new run starts and before each task execution
- Add a new parse function to extract the next unchecked task's text from `tasks.md`
- Show `implementation.log` as an entry in the artifact menu when the file exists
- Update the status bar to reflect the new log path

## Capabilities

### New Capabilities
- `implementation-log`: Log file storage in the change directory with run/task headers, and display as an artifact in the TUI

### Modified Capabilities
- `artifact-menu-view`: Add implementation.log as a file-based (non-schema) entry when it exists in the change directory

## Impact

- `src/runner.rs`: Log path construction, writing run and task headers into the log before spawning claude
- `src/data.rs`: New function to read the next unchecked task text from tasks.md
- `src/app.rs`: `build_artifact_menu_items()` adds implementation.log entry
- `src/ui.rs`: Status bar shows updated log path
