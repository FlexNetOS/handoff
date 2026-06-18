## 1. Config Extension

- [x] 1.1 Add `post_implementation_prompt` field to `TuiConfig` with `#[serde(default)]` defaulting to empty string
- [x] 1.2 Add `render_post_prompt(&self, name: &str) -> Option<String>` method that returns `None` if empty, otherwise replaces `{name}` and returns `Some`
- [x] 1.3 Add tests for new field: serialization roundtrip, partial deserialization defaults, `render_post_prompt` with and without placeholder

## 2. ImplUpdate and Implementation Loop

- [x] 2.1 Change `ImplUpdate::Finished` to `ImplUpdate::Finished { success: bool }` and update all match arms in `runner.rs`
- [x] 2.2 After the task loop completes successfully in `implementation_loop()`, check `config.render_post_prompt()` and execute the hook using `config.build_command()` with the same log file, cancel flag, and child handle
- [x] 2.3 Send `Finished { success: true }` only if both tasks and hook succeeded; send `Finished { success: false }` on any failure
- [x] 2.4 Add tests: loop sends `Finished { success: true }` when all tasks done and no hook configured; loop sends `Finished { success: false }` on process failure; hook failure results in `Finished { success: false }`

## 3. App Integration

- [x] 3.1 Update `poll_implementation()` to extract the `success` field from `ImplUpdate::Finished { success }` and pass it to `advance_batch()`
- [x] 3.2 Update `advance_batch()` to accept and use the `success` parameter directly instead of re-reading task progress from disk
- [x] 3.3 Add tests for `advance_batch()` with explicit success/failure parameter

## 4. Config Screen UI

- [x] 4.1 Add `post_implementation_prompt` as an editable field in the Config screen (same editing pattern as `prompt` — opens `$EDITOR` on Enter)
- [x] 4.2 Include the new field in save/load/reset-to-defaults flows
- [x] 4.3 Add tests for Config screen rendering and reset-to-defaults including the new field
