## 1. Core Implementation

- [x] 1.1 Add `openspec_command()` helper function to `src/data.rs` with `#[cfg(windows)]` and `#[cfg(not(windows))]` variants
- [x] 1.2 Replace `Command::new("openspec")` in `list_changes()` with `openspec_command()`
- [x] 1.3 Replace `Command::new("openspec")` in `get_change_status()` with `openspec_command()`

## 2. Testing

- [x] 2.1 Add unit test verifying `openspec_command()` returns a valid `Command` on the current platform
- [x] 2.2 Run existing tests to confirm no regressions
