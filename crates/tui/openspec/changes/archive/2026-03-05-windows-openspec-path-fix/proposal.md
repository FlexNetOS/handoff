## Why

On Windows, the TUI fails to find the `openspec` CLI because Rust's `Command::new("openspec")` does not resolve `.cmd` wrapper files. npm-installed tools on Windows use `.cmd` wrappers, so the command works in a terminal (where `cmd.exe` handles PATHEXT resolution) but fails when invoked directly from Rust.

## What Changes

- Add a platform-aware helper function that constructs the `openspec` `Command` correctly on both Windows and Unix.
- On Windows, invoke `openspec` via `cmd /c openspec` so that `.cmd` file resolution works.
- On Unix/macOS, keep the current direct `Command::new("openspec")` behavior.

## Capabilities

### New Capabilities
- `cross-platform-command`: Platform-aware construction of the `openspec` CLI command, ensuring `.cmd` wrapper resolution on Windows.

### Modified Capabilities

_(none)_

## Impact

- **Code**: `src/data.rs` — both `list_changes()` and `get_change_status()` will use the new helper instead of `Command::new("openspec")` directly.
- **Dependencies**: No new crate dependencies required.
- **Platforms**: Fixes Windows compatibility; no behavior change on Unix/macOS.
