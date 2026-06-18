## 1. Add file_path to ArtifactView

- [x]1.1 Add `file_path: Option<PathBuf>` field to `Screen::ArtifactView` variant in `src/app.rs`
- [x]1.2 Update all sites that construct `Screen::ArtifactView` to pass the file path (or `None` for non-file views like dependency graph text)
- [x]1.3 Add tests verifying ArtifactView is constructed with the correct file path

## 2. Implement refresh_screen method

- [x]2.1 Add `pub fn refresh_screen(&mut self)` method on `App` in `src/app.rs` that matches on `self.screen` and reloads data per variant
- [x]2.2 Implement ChangeList refresh: re-call `list_changes()` or `list_archived_changes()` based on active tab, rebuild `change_deps`, clamp selection
- [x]2.3 Implement ArtifactMenu refresh: re-call `get_change_status()` or `get_archived_change_status()`, rebuild menu items, clamp selection
- [x]2.4 Implement ArtifactView refresh: re-read file content using stored `file_path`, preserve scroll position
- [x]2.5 Implement DependencyView refresh: re-read dependencies, clamp selection
- [x]2.6 Implement DependencyGraph refresh: reload changes and deps, regenerate graph text, preserve scroll
- [x]2.7 Implement RunAllSelection refresh: rebuild entries from fresh change list, clamp selection
- [x]2.8 Implement DependencyAdd refresh: reload available changes, clamp selection
- [x]2.9 Add unit tests for `refresh_screen()` covering selection clamping when list shrinks

## 3. Wire up `r` key in event loop

- [x]3.1 Add `KeyCode::Char('r')` check in `run_app()` in `src/main.rs` as a global key (after `q` and `S` checks, before screen-specific match), calling `app.refresh_screen()`
- [x]3.2 Add test verifying `r` key is not handled when on Config screen
