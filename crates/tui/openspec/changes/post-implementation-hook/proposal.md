## Why

After a change is implemented (single or batch mode), there is no way to automatically run a follow-up action like committing the changes. Users must manually intervene between each change, breaking the automated flow. A configurable post-implementation prompt allows the batch runner to commit (or perform other actions) automatically after each successful implementation.

## What Changes

- Add a `post_implementation_prompt` field to `TuiConfig` (defaults to empty/disabled)
- Extend `ImplUpdate::Finished` to carry a `success: bool` flag so the caller knows whether implementation (and hook) succeeded
- After all tasks of a change complete successfully, run the post-implementation prompt using the existing `command` template
- If the post-implementation hook fails, treat the change as failed (abort batch)
- The hook runs for both single-change and batch implementations
- Add the new field to the Config screen for editing

## Capabilities

### New Capabilities
- `post-implementation-hook`: Configurable prompt that executes after successful change implementation, using the existing command template

### Modified Capabilities
- `tui-configuration`: Add `post_implementation_prompt` field to config loading, saving, defaults, and the Config screen
- `implementation-runner`: Extend the implementation loop to run the post-hook after success; extend `ImplUpdate::Finished` with success flag

## Impact

- `config.rs`: New field on `TuiConfig`, serialization, default handling
- `runner.rs`: Post-hook execution in `implementation_loop()`, `ImplUpdate::Finished` becomes `Finished { success: bool }`
- `app.rs`: `poll_implementation()` and `advance_batch()` use the success flag instead of re-reading task progress
- `ui.rs`: Config screen renders the new field
