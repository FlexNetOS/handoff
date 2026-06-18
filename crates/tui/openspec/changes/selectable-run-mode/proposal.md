## Why

The current implementation runner always executes Claude once per task, which reloads the full project context for every single task. For changes where all tasks share significant context, this wastes tokens. By allowing a per-change choice between the task-by-task runner and a single `opsx:apply` invocation, users can reduce token consumption when appropriate while keeping the fine-grained runner as the default.

## What Changes

- Rename `dependencies.yaml` to `change-config.yaml` across the entire codebase (data layer, app logic, tests). The file becomes a general per-change configuration file.
- Add a `run_mode` field to the per-change config (`"normal"` default, `"apply"` alternative).
- When `run_mode` is `"apply"`, pressing `R` launches a single `claude /opsx:apply <name>` process instead of the task-by-task loop. No task progress tracking or stall detection for this mode.
- Batch "Run All" respects each change's individual `run_mode`.
- The TUI allows toggling `run_mode` in the change's dependency/config view.

## Capabilities

### New Capabilities
- `per-change-config`: General per-change configuration file (`change-config.yaml`) replacing `dependencies.yaml`, supporting dependencies and run mode settings

### Modified Capabilities
- `implementation-runner`: Add apply run mode that launches a single `opsx:apply` invocation instead of the task-by-task loop

## Impact

- `data.rs`: `DependencyConfig` renamed to `ChangeConfig`, file path changes from `dependencies.yaml` to `change-config.yaml`, new `run_mode` field
- `runner.rs`: New `start_apply()` function for the apply mode, no stall detection or task tracking
- `app.rs`: `R` key checks `run_mode`, batch run dispatches per change mode, dependency view becomes change-config view with mode toggle
- `ui.rs`: Updated labels and key hints for the config/dependency view
- All existing tests referencing `dependencies.yaml` need updating
