## Context

The implementation runner (`src/runner.rs`) spawns `claude --print` for each unfinished task and redirects stdout/stderr to a log file. Currently, this log is written to `std::env::temp_dir()` (e.g., `/tmp/openspec-implement-{name}.log`), making it ephemeral and disconnected from the change. The TUI status bar shows the path but offers no way to view the log content.

## Goals / Non-Goals

**Goals:**
- Store the implementation log as `implementation.log` inside `openspec/changes/<name>/`
- Write clear separators into the log when a run starts and before each task
- Show the log as an entry in the artifact menu so users can read it in the TUI
- Parse the next unchecked task text from `tasks.md` for use in task headers

**Non-Goals:**
- Live-tailing / auto-refresh of log content in the viewer
- Log rotation or size management
- Structured/machine-readable log format

## Decisions

### Log path: change directory instead of temp dir
The log path changes from `std::env::temp_dir().join(...)` to `PathBuf::from("openspec/changes").join(name).join("implementation.log")`. This keeps the log alongside other artifacts and survives reboots.

**Alternative considered**: A dedicated `logs/` subdirectory — rejected as unnecessary complexity for a single file.

### Append-only with run headers
The log file is always opened in append mode. Each new run writes a clearly visible header block with timestamp and change name. Each task iteration writes a task header with task number and text. This makes it easy to visually scan the log even across multiple runs.

### Task text extraction via new `next_unchecked_task()` function
A new function in `data.rs` finds the first `- [ ]` line in `tasks.md` and returns both the task number (1-based index among all tasks) and the task text. The runner calls this before each claude invocation to write a meaningful task header.

### Log as file-based artifact menu entry
`build_artifact_menu_items()` in `app.rs` checks whether `implementation.log` exists in the change directory. If it does, it appends an entry at the bottom of the artifact list. This is independent of the OpenSpec schema — purely file-based detection.

**Alternative considered**: Registering the log as an OpenSpec artifact — rejected because it's not a schema-defined artifact and shouldn't participate in the dependency graph.

## Risks / Trade-offs

- [Large log files] → The log can grow large with many tasks/runs. Acceptable for V1; the existing artifact viewer handles scrolling. Future: could add tail-only viewing.
- [Log written during active run] → The artifact viewer reads the file once on open, so it shows a snapshot. Users must re-enter the view to see updates. Acceptable for V1.
- [Relative path assumption] → `PathBuf::from("openspec/changes/...")` assumes the TUI runs from the project root. This matches the existing runner behavior.
