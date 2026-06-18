## 1. Dependency Data Layer

- [x] 1.1 Add `DependencyConfig` struct with serde in `data.rs` for reading/writing `dependencies.yaml` (depends_on: Vec<String>), with `read_dependencies` and `write_dependencies` functions
- [x] 1.2 Add unit tests for reading/writing `dependencies.yaml` (file exists, file missing, empty list, add/remove entries)
- [x] 1.3 Implement topological sort using Kahn's algorithm in `data.rs` — takes a map of change names to their dependencies, returns sorted Vec or cycle error
- [x] 1.4 Add unit tests for topological sort (linear chain, multiple roots, diamond, no deps, cycle detection — direct and indirect)
- [x] 1.5 Add function to resolve archived dependencies — check `openspec/changes/archive/` for matching change names (with date-prefix stripping), returns set of fulfilled dependency names
- [x] 1.6 Add unit tests for archived dependency resolution (exact match, date-prefix match, no match)
- [x] 1.7 Add function to check if a change has a `tasks.md` file (used to filter eligible changes for batch runs)
- [x] 1.8 Add unit tests for tasks.md existence check

## 2. Dependency Management View

- [x] 2.1 Add `DependencyView` screen variant to `Screen` enum in `app.rs` with fields: change_name, dependencies list, selected index
- [x] 2.2 Add `DependencyAdd` screen variant to `Screen` enum for the "pick a change to add" selection list
- [x] 2.3 Implement `handle_dependency_view_input` in `app.rs` — navigation (j/k/Up/Down), remove (D), add (A opens DependencyAdd), back (Esc)
- [x] 2.4 Implement `handle_dependency_add_input` in `app.rs` — navigation, select with Enter (writes dependencies.yaml and returns to DependencyView), cancel with Esc
- [x] 2.5 Add unit tests for dependency view input handling (navigate, remove, add, Esc back)
- [x] 2.6 Implement `draw_dependency_view` in `ui.rs` — list of dependencies with selection highlight, keybinding hints
- [x] 2.7 Implement `draw_dependency_add` in `ui.rs` — selectable list of available changes
- [x] 2.8 Add unit tests for dependency view rendering

## 3. ArtifactMenu Dependencies Item

- [x] 3.1 Add "Dependencies [n]" menu item to `build_artifact_menu_items` in `app.rs` for active changes, reading dependency count from `dependencies.yaml`
- [x] 3.2 Handle Enter on Dependencies item to push DependencyView screen
- [x] 3.3 Add unit tests verifying Dependencies item appears for active changes and not for archived changes

## 4. ChangeList Inline Dependencies

- [x] 4.1 Read dependencies for all active changes in the ChangeList and pass them to the UI rendering
- [x] 4.2 Modify `draw_change_list` in `ui.rs` to display dependency info right-aligned (e.g., `<- dep1, dep2`) with truncation for long lists
- [x] 4.3 Add unit tests for inline dependency display rendering

## 5. Dependency Graph View

- [x] 5.1 Add `DependencyGraph` screen variant to `Screen` enum with scroll support
- [x] 5.2 Implement ASCII graph generation function in `data.rs` — takes changes and their deps, produces multi-line string with tree connectors
- [x] 5.3 Add unit tests for graph generation (linear, diamond, no deps, multiple roots)
- [x] 5.4 Implement `handle_dependency_graph_input` in `app.rs` — scroll (j/k/Up/Down), back (Esc)
- [x] 5.5 Implement `draw_dependency_graph` in `ui.rs`
- [x] 5.6 Add `G` keybinding to ChangeList (Active tab only) to open the dependency graph view

## 6. Run All Selection Screen

- [x] 6.1 Add `RunAllSelection` screen variant to `Screen` enum with fields: list of (change_name, selected, blocked, progress) entries
- [x] 6.2 Implement function to build the selection list — filter changes with tasks.md, read dependencies, determine blocked state based on excluded changes
- [x] 6.3 Implement `handle_run_all_selection_input` in `app.rs` — navigation (j/k/Up/Down), toggle (Space), start (Enter with cycle check), cancel (Esc)
- [x] 6.4 Add unit tests for selection input handling (toggle, blocked propagation, cycle detection on Enter)
- [x] 6.5 Implement `draw_run_all_selection` in `ui.rs` — checkboxes [x]/[ ]/[~], change names, progress, blocked reasons
- [x] 6.6 Add `A` keybinding to ChangeList (Active tab only, no running implementation) to open Run All selection

## 7. Batch Runner

- [x] 7.1 Add `BatchImplState` struct in `runner.rs` with queue, current_index, failed/skipped/completed sets
- [x] 7.2 Implement batch advancement logic — when current ImplState finishes, determine next eligible change considering failures and skips
- [x] 7.3 Add unit tests for batch advancement (success advances, failure skips dependents, all complete finishes batch)
- [x] 7.4 Integrate batch state into `App` — add `batch: Option<BatchImplState>` field, wire up batch start from RunAllSelection
- [x] 7.5 Handle ImplUpdate::Finished in the event loop to trigger batch advancement — start next change or clear batch state
- [x] 7.6 Add cancellation support — `S` during batch run stops current change and clears batch state
- [x] 7.7 Add unit tests for batch cancellation and integration with app state

## 8. Status Bar Batch Progress

- [x] 8.1 Modify `draw_status_bar` in `ui.rs` to show batch progress when `BatchImplState` is active (current change X/Y, failed/skipped counts)
- [x] 8.2 Add unit tests for batch status bar rendering (normal progress, with failures, with skips)
