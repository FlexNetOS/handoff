## 1. Config

- [x] 1.1 Add `run_finished_command: String` field to `TuiConfig` in `config.rs` with `#[serde(default)]` and empty string default
- [x] 1.2 Add unit tests for serialization/deserialization of `run_finished_command` (roundtrip, missing field defaults to empty, partial config)

## 2. Config Screen

- [x] 2.1 Add `RunFinishedCommand` variant to `ConfigField` enum and add `run_finished_command` field to `Screen::Config` in `app.rs`
- [x] 2.2 Update Tab cycle in `handle_config_input` to include `RunFinishedCommand` between `InteractiveCommand` and `Command`
- [x] 2.3 Update Enter handling in `handle_config_input` to allow inline editing for `RunFinishedCommand` (same as `Command` field)
- [x] 2.4 Update Save (`S`) and Reset (`D`) handlers to include `run_finished_command`
- [x] 2.5 Update `open_config_screen` to initialize `run_finished_command` from config
- [x] 2.6 Render `RunFinishedCommand` field in Config screen in `ui.rs`
- [x] 2.7 Add unit tests for Config screen Tab navigation, inline editing, save, and reset including the new field

## 3. Run Finished Hook

- [x] 3.1 Add a helper method `spawn_run_finished_command(&self)` on `App` that spawns the configured command via `sh -c` (or `cmd /C` on Windows) fire-and-forget, doing nothing if the field is empty
- [x] 3.2 Call `spawn_run_finished_command` in `poll_implementation` when a run ends and `advance_batch` does not start a new run (single run done or batch fully exhausted)
- [x] 3.3 Add unit tests: helper does nothing when command is empty; hook is not called mid-batch; hook is called when batch is exhausted; hook is called for single run finish
