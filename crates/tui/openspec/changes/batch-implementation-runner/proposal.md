## Why

Currently the implementation runner can only execute a single change at a time, requiring the user to manually start each change individually. When working with multiple interdependent changes, the user must track execution order and dependencies themselves. A batch runner with dependency awareness would automate this workflow, allowing the user to run all changes in the correct order with a single action.

## What Changes

- Add `dependencies.yaml` file support per change, defining which changes must complete before this one runs
- Add a dependency management view accessible from the ArtifactMenu, allowing users to add/remove dependencies
- Display dependency information inline in the ChangeList view
- Add a "Run All" selection screen triggered from ChangeList, showing all changes with `tasks.md`, allowing exclusion of individual changes
- Extend the runner to support batch execution: sequential processing in topological order, skipping completed changes, propagating failures transitively through the dependency graph while continuing with independent changes
- Add a dependency graph view accessible from the ChangeList
- Extend the status bar to show batch progress (current change, overall change count, failed/skipped counts)
- Treat archived changes as fulfilled dependencies

## Capabilities

### New Capabilities
- `change-dependencies`: Reading, writing, and managing `dependencies.yaml` files per change, including topological sorting and cycle detection
- `dependency-management-view`: UI screen for viewing, adding, and removing dependencies on a per-change basis
- `batch-runner`: Sequential execution of multiple changes respecting dependency order, with failure propagation and skip logic
- `run-all-selection`: Selection screen for choosing which changes to include in a batch run, with blocked-state visualization

### Modified Capabilities
- `change-list-view`: Display dependency information inline next to each change, add keybinding for "Run All" and dependency graph view
- `implementation-runner`: Extend runner to support batch mode with `BatchImplState` tracking multiple changes
- `implementation-status-bar`: Show batch progress information (current change X/Y, failed/skipped counts)
- `artifact-menu-view`: Add "Dependencies" menu item showing dependency count

## Impact

- **runner.rs**: New `BatchImplState` struct and batch execution loop
- **data.rs**: Functions for reading/writing `dependencies.yaml`, topological sort, cycle detection
- **app.rs**: New screens (DependencyView, RunAllSelection, DependencyGraph), extended input handling, batch state management
- **ui.rs**: Rendering for new screens, inline dependency display in ChangeList, extended status bar
- **No new dependencies**: Topological sort and cycle detection implemented with standard library
