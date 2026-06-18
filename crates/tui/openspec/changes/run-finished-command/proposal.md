## Why

Users start long-running implementation runs (single or batch) and switch to other work. Currently there is no way to get notified when the run finishes. A configurable shell command that fires once the entire run is done lets users set up push notifications (e.g., ntfy, notify-send) without polling the TUI.

## What Changes

- Add a new `run_finished_command` field to `TuiConfig` (default: empty string = disabled)
- After all runs complete (single run or entire batch), spawn the configured shell command as a fire-and-forget child process
- The command fires regardless of success, failure, or stall — the user just wants to know "it's done"
- Add a Config screen field so the user can edit `run_finished_command` from the TUI

## Capabilities

### New Capabilities
- `run-finished-notification`: Configurable shell command executed once when all implementation runs complete

### Modified Capabilities
- `tui-configuration`: Add `run_finished_command` field to config loading, saving, defaults, and the Config screen

## Impact

- `config.rs`: New field + helper method
- `app.rs`: Spawn command when run/batch ends in `poll_implementation`/`advance_batch`
- `ui.rs`: Render new field in Config screen
- `app.rs` Config screen input handling: Support editing the new field
