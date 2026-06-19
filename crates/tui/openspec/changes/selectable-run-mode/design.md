## Context

The TUI currently manages per-change dependencies via `dependencies.yaml` containing a single `depends_on` list. The implementation runner (`runner.rs`) always uses a task-by-task loop: it parses `tasks.md`, spawns the configured command once per unchecked task, tracks progress, and detects stalls.

Users want an alternative: launching a single `opsx:apply` invocation that handles all tasks in one Claude session, saving tokens by not reloading context per task. This requires a per-change configuration to select the run mode.

## Goals / Non-Goals

**Goals:**
- Rename `dependencies.yaml` to `change-config.yaml` as a general per-change config file
- Add `run_mode` field with values `"normal"` (default) and `"apply"`
- Implement apply mode: single subprocess, no task tracking, no stall detection
- Support mixed modes in batch runs (each change uses its own `run_mode`)
- Allow toggling `run_mode` in the TUI dependency view

**Non-Goals:**
- Backward compatibility with `dependencies.yaml` (single user, not needed)
- Progress tracking for apply mode (the apply skill manages its own flow)
- Additional run modes beyond `normal` and `apply`

## Decisions

### 1. Unified per-change config file

**Decision**: Rename `dependencies.yaml` → `change-config.yaml` and expand `DependencyConfig` → `ChangeConfig`.

```rust
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ChangeConfig {
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub run_mode: RunMode,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum RunMode {
    #[default]
    Normal,
    Apply,
}
```

**Rationale**: A single config file per change is simpler than multiple files. The `RunMode` enum with serde defaults means existing changes without the field automatically get `normal` mode. Using an enum instead of a string ensures compile-time validation.

### 2. Apply mode implementation

**Decision**: Add `start_apply()` in `runner.rs` that spawns a single subprocess running `claude /opsx:apply <name>`.

The apply mode:
- Uses `TuiConfig::build_command()` with `/opsx:apply <name>` as the prompt
- Spawns one subprocess, redirects output to `implementation.log`
- Sends only `Finished` (no `Progress` updates, no `Stalled`)
- Supports cancellation via the same `cancel_flag` + `child_handle` mechanism

**Rationale**: Reusing the existing `ImplState` struct and cancellation mechanism means the TUI's poll/stop logic works unchanged. The only difference is what runs inside the thread.

### 3. Dispatch at run trigger

**Decision**: When `R` is pressed or batch advances, read `run_mode` from the change's `change-config.yaml` and dispatch to either `start_implementation()` or `start_apply()`.

```
R pressed → read_change_config(change_dir) → match run_mode {
    Normal → runner::start_implementation(name, config)
    Apply  → runner::start_apply(name, config)
}
```

**Rationale**: The dispatch point is in `app.rs` where the run is triggered. Both modes return `ImplState`, so the rest of the app (polling, stop, status bar) works identically.

### 4. Data layer changes

**Decision**: Rename functions to reflect the broader scope:
- `read_dependencies()` → `read_change_config()` (returns `ChangeConfig`)
- `write_dependencies()` → `write_change_config()` (takes `&ChangeConfig`)
- `load_change_dependencies()` stays but calls `read_change_config()` internally
- Add `read_run_mode()` convenience function

**Rationale**: Callers that only need dependencies can extract from `ChangeConfig`. A dedicated `read_run_mode()` keeps the dispatch code in `app.rs` clean.

### 5. UI for toggling run mode

**Decision**: Add `M` keybinding in `DependencyView` to toggle between Normal and Apply. Display current mode in the view header or as a status line.

**Rationale**: The dependency view already shows per-change config. Adding a toggle there keeps related settings together without needing a new screen.

## Risks / Trade-offs

- **Apply mode gives no progress feedback** → User sees "running..." with no task count. Acceptable because the log view still shows live output.
- **Apply mode has no stall detection** → If the apply process hangs, the user must manually stop it with `S`. This is expected since apply manages its own flow.
- **Renaming the config file** → All existing `dependencies.yaml` files become orphaned. Acceptable for single-user project.
