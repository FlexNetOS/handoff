## 1. Task Text Parsing

- [x] 1.1 Add `next_unchecked_task()` function to `data.rs` that reads tasks.md and returns the 1-based index and description text of the first `- [ ]` task (returns `None` if all complete)
- [x] 1.2 Add tests for `next_unchecked_task()`: mixed tasks, all complete, no tasks, missing file

## 2. Log Path and Headers

- [x] 2.1 Change `log_path` in `runner::start_implementation()` from `std::env::temp_dir()` to `openspec/changes/<name>/implementation.log`
- [x] 2.2 Write a run header (separator lines, "IMPLEMENTATION RUN STARTED", timestamp, change name) into the log at the start of `implementation_loop` before entering the task loop
- [x] 2.3 Write a task header (separator line, task number N/M, task description) into the log before each claude invocation using `next_unchecked_task()`
- [x] 2.4 Update existing tests in `runner.rs` that reference the old `/tmp/` log path

## 3. Artifact Menu Integration

- [x] 3.1 Extend `build_artifact_menu_items()` in `app.rs` to check for `implementation.log` in the change directory and append an "Implementation Log" entry if it exists
- [x] 3.2 Add tests for `build_artifact_menu_items()` with and without `implementation.log` present

## 4. Status Bar Update

- [x] 4.1 Update the status bar log path display in `ui.rs` to reflect the new change-local path
- [x] 4.2 Update any status bar tests that assert on the old temp-dir log path
