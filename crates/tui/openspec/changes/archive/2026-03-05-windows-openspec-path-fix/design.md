## Context

The TUI application invokes the `openspec` CLI via `std::process::Command::new("openspec")` in `src/data.rs`. On Unix this works because the binary is resolved directly via PATH. On Windows, npm-installed tools are wrapped in `.cmd` files. Rust's `Command` does not resolve `.cmd` extensions through PATHEXT, so the command fails with "not found" even when the user can run `openspec` from their terminal.

## Goals / Non-Goals

**Goals:**
- Make `openspec` CLI invocation work on Windows where it is installed as a `.cmd` wrapper (e.g., via npm).
- Keep existing Unix/macOS behavior unchanged.
- Centralize command construction so all call sites use the same logic.

**Non-Goals:**
- Supporting custom `openspec` binary paths or environment variable overrides.
- Bundling or embedding the `openspec` CLI.

## Decisions

### Use `cmd /c openspec` on Windows

**Decision**: On Windows, construct the command as `cmd.exe /C openspec` instead of `Command::new("openspec")`.

**Rationale**: `cmd.exe` natively resolves `.cmd` and `.bat` files via PATHEXT. This is the simplest approach with zero additional dependencies.

**Alternatives considered**:
- **`which` crate**: Adds a dependency just to locate the binary. Overkill for this case.
- **`OPENSPEC_PATH` env var**: Requires user configuration. Poor DX for a tool that should just work.
- **Hardcoded `.cmd` extension**: Brittle — doesn't account for `.exe` installations or other packaging.

### Introduce a helper function `openspec_command()`

**Decision**: Add a single function `fn openspec_command() -> Command` in `src/data.rs` that encapsulates the platform logic using `#[cfg(windows)]` / `#[cfg(not(windows))]`.

**Rationale**: Both `list_changes()` and `get_change_status()` call `openspec`. A shared helper eliminates duplication and ensures consistency.

## Risks / Trade-offs

- **`cmd /c` overhead on Windows**: Spawning `cmd.exe` adds minimal overhead (~5ms). Acceptable since these are infrequent CLI calls, not hot-path operations. → No mitigation needed.
- **`cmd /c` argument escaping**: Arguments containing special shell characters could be misinterpreted by `cmd.exe`. → Current arguments (`list`, `status`, `--change`, `--json`) are all simple ASCII strings with no special characters. Not a risk for current usage.
