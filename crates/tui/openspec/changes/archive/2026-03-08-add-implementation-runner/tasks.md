## 1. Data Layer: Task Parsing and Claude Command

- [x] 1.1 Add function `parse_task_progress(path) -> (completed, total)` to `data.rs` that counts `- [x]` and `- [ ]` lines in a tasks.md file
- [x] 1.2 Add function `claude_command() -> Command` to `data.rs` following the cross-platform pattern (cmd wrapper on Windows) for invoking the `claude` CLI
- [x] 1.3 Add tests for `parse_task_progress` with various task states (all done, none done, mixed)

## 2. Implementation Runner Module

- [x] 2.1 Create `src/runner.rs` module with `ImplState` struct holding: change_name, completed, total, log_path, receiver, cancel_flag, child_handle
- [x] 2.2 Implement `start_implementation(change_name) -> ImplState` that spawns a worker thread running the Claude loop: grep for unchecked tasks, invoke claude with `--print --dangerously-skip-permissions`, redirect output to log file, send progress updates via mpsc channel
- [x] 2.3 Implement `stop_implementation(state)` that sets the AtomicBool cancel flag and kills the active child process via the shared Arc<Mutex<Option<Child>>>
- [x] 2.4 Add `ImplUpdate` enum for channel messages: `Progress { completed, total }` and `Finished`
- [x] 2.5 Add tests for the runner: verify cancel flag stops the loop, verify progress counting

## 3. App State Integration

- [x] 3.1 Add `pub implementation: Option<ImplState>` field to `App` struct in `app.rs`
- [x] 3.2 Handle `R` key in `handle_artifact_menu_input`: start implementation if none running, using the current change_name
- [x] 3.3 Handle `S` key globally in the event loop (all screens): stop implementation if running
- [x] 3.4 Add `poll_implementation(&mut self)` method that checks the mpsc receiver for updates and updates ImplState or clears it on Finished
- [x] 3.5 Add tests for R key starting implementation and S key stopping it

## 4. Event Loop: Switch to Polling

- [x] 4.1 Change `event::read()` in `main.rs` to `event::poll(Duration::from_millis(500))` + `event::read()` pattern
- [x] 4.2 Call `app.poll_implementation()` on each loop iteration (after poll returns, whether or not a key event was received)
- [x] 4.3 Verify TUI remains responsive with manual testing

## 5. Status Bar UI

- [x] 5.1 Add `draw_status_bar(frame, impl_state, area)` function in `ui.rs` that renders change name, progress counts, progress bar, log path, and `[S] Stop` hint
- [x] 5.2 Modify `draw()` in `ui.rs` to split layout when `app.implementation.is_some()`: main content area + 2-line bottom bar
- [x] 5.3 Add tests for status bar rendering: verify progress bar, task counts, stop hint, and change name are displayed
- [x] 5.4 Add test that layout is not split when no implementation is running
