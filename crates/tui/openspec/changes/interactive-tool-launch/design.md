## Context

The TUI currently launches AI tools only in non-interactive batch mode (`--print`), capturing stdout/stderr to a log file while the TUI continues rendering. An existing pattern for suspending the TUI already exists: `edit_in_external_editor` in `main.rs` suspends the terminal, launches `$EDITOR`, and restores the TUI afterward.

Users want to launch their tool interactively — the same suspend/restore pattern, but with a configurable command instead of `$EDITOR`.

## Goals / Non-Goals

**Goals:**
- Add `interactive_command` field to `TuiConfig` with default `"claude"`
- Launch the configured command via `I` keybinding, suspending the TUI
- Make the field editable in the Config screen
- Support whitespace-split command with arguments (e.g., `"claude --model opus"`)

**Non-Goals:**
- No prompt injection or `{name}`/`{prompt}` placeholders — this is a raw command launch
- No output capture or progress tracking
- No integration with the implementation runner or run modes

## Decisions

### 1. Config field design

**Decision**: Add `interactive_command: String` to `TuiConfig` with default `"claude"`.

```rust
#[serde(default = "default_interactive_command")]
pub interactive_command: String,
```

The field stores the full command string (binary + optional args), split on whitespace at launch time. This mirrors how the existing `command` field works.

**Rationale**: Consistent with the existing `command` field pattern. A single string is simpler than separate binary/args fields and covers the common cases (`"claude"`, `"claude --model opus"`, `"aider"`).

### 2. Launch mechanism

**Decision**: Reuse the suspend/restore pattern from `edit_in_external_editor` in `main.rs`. Split the `interactive_command` on whitespace, use the first token as the binary and the rest as args. Call `.status()` (blocking) instead of `.spawn()`.

```
LeaveAlternateScreen + disable_raw_mode
    → Command::new(binary).args(args).status()
    → enable_raw_mode + EnterAlternateScreen + clear
```

**Rationale**: The pattern is proven, handles terminal restore correctly (including on panics), and `.status()` blocks until the user exits the interactive session.

### 3. Keybinding and signaling

**Decision**: `I` in the change list view (Active tab only). `App::handle_list_input` returns a signal enum variant (similar to how Enter on Prompt signals the editor). `main.rs` handles the actual terminal suspend/launch/restore.

**Rationale**: `app.rs` must not depend on terminal details. The signal pattern keeps the separation clean — app handles logic, main handles terminal.

### 4. Config screen integration

**Decision**: Add `interactive_command` as a new field in the Config screen, editable inline (same as the existing `command` field). Tab cycles through Command → Prompt → Post-Implementation Prompt → Interactive Command.

**Rationale**: Inline editing is appropriate for a short command string. Consistent with how `command` is edited.

## Risks / Trade-offs

- **Terminal state corruption if interactive process crashes** → The restore sequence in `main.rs` already handles this for `$EDITOR`. Same approach applies here.
- **Command not found** → `.status()` returns an error, TUI restores normally. No special handling needed beyond what the terminal restore already provides.
- **No cross-platform `cmd /C` wrapper** → Unlike the batch `command` field which goes through `build_command()`, the interactive command is run directly. On Windows, if the tool is an npm `.cmd` wrapper, it may not resolve. Acceptable for now — same limitation as `$EDITOR`.
