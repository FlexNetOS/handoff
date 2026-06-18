## Context

The TUI currently supports running a single change's implementation via `runner::start_implementation()`, which spawns a worker thread iterating through tasks in one `tasks.md`. The `App` struct holds `Option<ImplState>` for tracking a single running implementation.

Users working with multiple changes must manually determine execution order and start each change individually. Changes often have implicit dependencies (e.g., "add-user-model" must complete before "add-auth-layer").

## Goals / Non-Goals

**Goals:**
- Allow users to run all eligible changes sequentially with one action
- Support explicit dependency declarations between changes via `dependencies.yaml`
- Automatically determine execution order via topological sort
- Handle failures gracefully: skip dependent changes, continue with independent ones
- Provide UI for managing dependencies and selecting changes for batch runs
- Display batch progress in the status bar
- Visualize the dependency graph

**Non-Goals:**
- Parallel execution of changes (too risky with shared codebase)
- Auto-detection of dependencies (users declare them explicitly)
- Dependency management for archived changes (they are read-only)
- Integration with external dependency management systems

## Decisions

### Decision 1: Dependencies stored in `dependencies.yaml` per change

Each change can have an optional `openspec/changes/<name>/dependencies.yaml`:

```yaml
depends_on:
  - add-api
  - add-user-model
```

**Rationale:** Per-change files get archived with the change, keeping history intact. A central file would require cleanup when changes are archived. The format is simple and human-editable.

**Alternative considered:** Storing in `openspec/config.yaml` centrally — rejected because it couples change lifecycle to config management and doesn't archive cleanly.

### Decision 2: Topological sort using Kahn's algorithm

Kahn's algorithm processes nodes with zero in-degree first, naturally producing a valid execution order. It also detects cycles: if the algorithm terminates before processing all nodes, a cycle exists.

**Rationale:** Kahn's is iterative (no recursion depth concerns), naturally produces the sorted order as output, and cycle detection falls out for free. Implementation is straightforward with `HashMap` and `VecDeque`.

**Alternative considered:** DFS-based topological sort — works but requires separate cycle detection logic.

### Decision 3: Batch state wraps existing ImplState

```
BatchImplState {
    queue: Vec<String>,        // Topologically sorted change names
    current_index: usize,      // Index into queue
    failed: HashSet<String>,   // Changes that failed
    skipped: HashSet<String>,  // Changes skipped due to failed deps
    completed: HashSet<String>,// Successfully completed changes
}
```

The existing `App.implementation: Option<ImplState>` remains for tracking the currently running single change. A new `App.batch: Option<BatchImplState>` tracks the overall batch progress. When a change finishes, the batch state advances to the next eligible change.

**Rationale:** Minimal changes to the existing runner. `ImplState` already handles single-change execution well. The batch layer orchestrates which change runs next.

**Alternative considered:** Merging batch and single into one state — rejected because it would require rewriting the existing runner and break the single-change use case.

### Decision 4: Failure propagation via transitive dependency check

When a change fails, before starting each subsequent change, check if any of its dependencies (transitively) are in the `failed` or `skipped` sets. If so, add it to `skipped`.

**Rationale:** Simple to implement — just walk the dependency graph from each candidate change. No need for pre-computation since the number of changes is small (typically < 20).

### Decision 5: New screens for batch workflow

Three new `Screen` variants:

- **`RunAllSelection`**: Checkbox list of eligible changes, space to toggle, shows blocked state when dependencies are excluded. Triggered by `A` from ChangeList.
- **`DependencyView`**: List of dependencies for a change, with add/remove actions. Triggered from ArtifactMenu.
- **`DependencyGraph`**: Read-only ASCII graph showing all changes and their dependency relationships. Triggered by `G` from ChangeList.

All follow the existing push/pop screen stack pattern.

### Decision 6: Archived changes as fulfilled dependencies

When resolving dependencies, check both active changes (completed tasks == total tasks) and archived change names. An archived change is always considered fulfilled.

**Rationale:** Archived changes represent completed work. The user shouldn't need to keep old changes active just to satisfy dependency resolution.

### Decision 7: Inline dependency display in ChangeList

Show dependencies as a right-aligned column in the ChangeList, e.g.:

```
  add-api                    [2/5]
  add-auth-layer             [0/7]   <- add-api, add-user-model
```

**Rationale:** Always visible without extra navigation. Fits the existing layout pattern of change name + progress on the right.

## Risks / Trade-offs

- **[Risk] Dependency on non-existent change** -> Show warning in DependencyView and RunAllSelection. Allow the run but treat as unfulfilled (skip the dependent change).
- **[Risk] User excludes a change that others depend on** -> Show blocked indicator `[~]` in RunAllSelection. Do not prevent the run, but clearly communicate which changes will be skipped.
- **[Risk] Large dependency graphs clutter ChangeList** -> Truncate long dependency lists with "..." if they exceed available width. Full list visible in DependencyView.
- **[Trade-off] Single batch at a time** -> Only one batch run can be active. Starting a new batch while one runs is not allowed (same constraint as single runs today).
