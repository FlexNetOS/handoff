## Why

The TUI currently only allows browsing changes and artifacts, but cannot trigger implementation. Users have to manually run a shell script (`ralph-implement.sh`) to loop through tasks with Claude. Integrating this directly into the TUI would provide a seamless workflow: browse a change, press a hotkey, and watch tasks get implemented automatically — all without leaving the application.

## What Changes

- Add a background implementation runner that spawns Claude in a loop to implement tasks from a change's `tasks.md`
- Add a persistent status bar at the bottom of the screen showing implementation progress (completed/total tasks, progress bar)
- Add hotkey `R` on the ArtifactMenu screen to start the implementation runner for the current change
- Add a hotkey to stop a running implementation
- Claude output is redirected to a log file for later inspection
- The TUI remains fully usable while implementation runs in a background thread

## Capabilities

### New Capabilities
- `implementation-runner`: Background task runner that calls `claude --print --dangerously-skip-permissions` in a loop, implementing one task per iteration and tracking progress via tasks.md checkbox state
- `implementation-status-bar`: Persistent bottom bar showing implementation progress across all screens, with progress bar, task counts, and stop control

### Modified Capabilities

## Impact

- `src/app.rs`: New `implementation` field on `App` struct, new hotkey handler for `R`, progress polling logic
- `src/main.rs`: Event loop needs non-blocking polling to update progress bar while implementation runs
- `src/ui.rs`: Layout split to reserve bottom rows for status bar when implementation is active
- `src/data.rs`: New function to parse task completion from tasks.md, new function to spawn Claude process
- New dependency: None expected (std::thread, std::sync::mpsc, std::process are all in stdlib)
