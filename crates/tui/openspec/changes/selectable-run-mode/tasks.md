## 1. Data Layer: Rename and extend config struct

- [x] 1.1 In `data.rs`, rename `DependencyConfig` to `ChangeConfig`, add `RunMode` enum (Normal/Apply with serde defaults), change file path from `dependencies.yaml` to `change-config.yaml`
- [x] 1.2 Rename `read_dependencies()` to `read_change_config()` returning `ChangeConfig`, update `write_dependencies()` to `write_change_config()` taking `&ChangeConfig`, add `read_run_mode()` convenience function
- [x] 1.3 Update `load_change_dependencies()` to use `read_change_config()` internally
- [x] 1.4 Update all existing tests in `data.rs` that reference `dependencies.yaml` or `DependencyConfig` to use the new names and file path
- [x] 1.5 Add tests for `RunMode` deserialization: missing field defaults to Normal, explicit `apply` parses correctly, roundtrip serialization

## 2. Runner: Add apply mode

- [x] 2.1 Add `start_apply()` function in `runner.rs` that spawns a single subprocess with `/opsx:apply <name>` as prompt, redirects output to `implementation.log`, sends only `Finished` on completion, and supports cancellation
- [x] 2.2 Add tests for `start_apply()`: verify log path is set, verify cancel flag works, verify no Progress or Stalled messages are sent

## 3. App: Dispatch by run mode

- [x] 3.1 In `app.rs`, update `R` key handler in `handle_artifact_menu_input()` to read `run_mode` from `change-config.yaml` and dispatch to `start_implementation()` or `start_apply()`
- [x] 3.2 Update batch run logic (`handle_run_all_input()` and `advance_batch()`) to read each change's `run_mode` and dispatch accordingly
- [x] 3.3 Update `handle_dependency_view_input()`: add `run_mode` field to `DependencyView` screen variant, add `M` keybinding to toggle run mode and persist to `change-config.yaml`
- [x] 3.4 Update `handle_dependency_add_input()` and related methods to pass `ChangeConfig` instead of just dependencies
- [x] 3.5 Update all existing tests in `app.rs` that reference `dependencies.yaml`, `write_dependencies`, or `read_dependencies` to use new function names and file path
- [x] 3.6 Add tests for `R` key dispatching to apply mode, `M` key toggling run mode, and batch run with mixed modes

## 4. UI: Display run mode

- [x] 4.1 Update `draw_dependency_view()` in `ui.rs` to display current run mode and `M` key hint
- [x] 4.2 Update existing UI tests for dependency view to account for new run mode display and key hint
