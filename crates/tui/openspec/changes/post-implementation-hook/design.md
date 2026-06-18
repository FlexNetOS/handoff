## Context

The TUI currently runs implementation tasks via `implementation_loop()` in a worker thread, which spawns a configurable command per unfinished task. After all tasks complete, it sends `ImplUpdate::Finished` to the main thread. In batch mode, `advance_batch()` then determines success by re-reading task progress and starts the next change.

There is no mechanism to run a follow-up action after a change is implemented. Users who want to commit after each change must do so manually, breaking the automated batch flow.

## Goals / Non-Goals

**Goals:**
- Run a configurable post-implementation prompt after successful change implementation
- Use the existing `command` template for execution (same binary, different prompt)
- Support `{name}` placeholder in the post-implementation prompt
- Abort on hook failure (both single and batch mode)
- Carry success/failure information in `ImplUpdate::Finished`

**Non-Goals:**
- Separate command template for post-hook (reuses existing `command`)
- Running hooks on failed implementations
- Multiple hooks or hook chaining
- Async/parallel hook execution

## Decisions

### 1. Extend `ImplUpdate::Finished` with `success: bool`

Currently `ImplUpdate::Finished` is a unit variant. Change it to `Finished { success: bool }`.

**Rationale**: The worker thread already knows whether implementation succeeded (all tasks done) or failed (process error, cancellation). Carrying this in the message eliminates the need for `advance_batch()` to re-read task progress from disk, and it naturally includes hook failure information.

**Alternative considered**: Separate `ImplUpdate::HookFailed` variant. Rejected because it adds complexity without benefit — the caller only needs to know "did everything succeed?".

### 2. Run the post-hook inside `implementation_loop()`

After the task loop completes successfully (all tasks checked off), the worker thread checks if `post_implementation_prompt` is non-empty. If so, it renders `{name}`, builds the command via `config.build_command()`, and spawns the process. The hook's stdout/stderr go to the same log file.

**Rationale**: Keeps the hook execution in the same thread, reuses the existing process spawning infrastructure, and means `Finished { success }` already reflects hook outcome. The status bar stays active during hook execution.

**Alternative considered**: Running the hook from the main thread between `Finished` and `advance_batch()`. Rejected because it would block the TUI event loop.

### 3. Add `post_implementation_prompt` to `TuiConfig`

A single new field with `#[serde(default)]` defaulting to empty string. Empty means no hook runs.

```yaml
command: "claude --print --dangerously-skip-permissions {prompt}"
prompt: "Read ... implement {name}"
post_implementation_prompt: "Commit all changes with a meaningful message"
```

**Rationale**: Simpler than a separate command+prompt pair. The `command` template is already configurable, so the user controls the binary. The post-hook only needs a different prompt.

### 4. Add `render_post_prompt()` to `TuiConfig`

New method that replaces `{name}` in `post_implementation_prompt`, mirroring `render_prompt()`. Returns `Option<String>` — `None` if the prompt is empty.

### 5. Config screen: add editable field

The Config screen gets a third field for `post_implementation_prompt`, editable like the prompt field (opens `$EDITOR` on Enter).

## Risks / Trade-offs

- **[Hook hangs]** → The cancellation mechanism (cancel flag + child kill) applies equally to the hook process. User can press `S` to stop.
- **[Log interleaving]** → Hook output goes to the same log file as implementation output. This is intentional — it provides a complete record.
- **[Breaking change to ImplUpdate]** → All match arms on `ImplUpdate::Finished` must be updated. This is internal API only, caught at compile time.
