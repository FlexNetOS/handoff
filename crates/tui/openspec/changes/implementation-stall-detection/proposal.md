## Why

The implementation runner loops indefinitely when Claude exits without completing any tasks (e.g., due to token exhaustion or repeated errors). The user must manually press 'S' to stop, otherwise the runner keeps spawning new Claude processes with no progress. An automatic stall detection prevents wasted resources and gives clear feedback.

## What Changes

- Add a stall counter to the implementation loop that tracks consecutive runs without task progress
- After 3 consecutive runs with no new tasks completed (regardless of exit code), automatically abort the change
- Add a new `ImplUpdate::Stalled` variant so the UI can distinguish between successful completion and stall-abort
- Treat stalled changes as failures in the batch runner (dependents get skipped)
- Remove the immediate abort on non-zero exit code — instead count failed runs toward the stall counter like successful-but-unproductive runs

## Capabilities

### New Capabilities
- `stall-detection`: Automatic detection and abort when the implementation runner makes no progress after consecutive attempts

### Modified Capabilities
- `implementation-runner`: The loop no longer aborts immediately on process failure; instead it tracks consecutive no-progress runs and aborts after 3

## Impact

- `runner.rs`: Modified `implementation_loop` control flow, new `ImplUpdate::Stalled` variant
- `app.rs`: Handle `ImplUpdate::Stalled` in the event loop and `advance_batch` (treat as failure)
- `ui.rs`: Potentially show "stalled" indicator in status bar
