## 1. ImplUpdate Enum Extension

- [x] 1.1 Add `Stalled` variant to `ImplUpdate` enum in `runner.rs`
- [x] 1.2 Add tests for `ImplUpdate::Stalled` message passing via channel

## 2. Stall Detection in Implementation Loop

- [x] 2.1 Add stall counter (`stall_count`) and `prev_completed` tracking to `implementation_loop`, increment on no-progress runs, reset on progress, abort and send `Stalled` when counter reaches 3
- [x] 2.2 Remove the immediate abort on `!exited_ok` — instead re-read progress after failed runs and count toward stall counter like successful runs
- [x] 2.3 Add test: loop sends `Stalled` after 3 consecutive no-progress runs
- [x] 2.4 Add test: stall counter resets when a task is completed between failed runs
- [x] 2.5 Add test: loop continues after 1-2 failed runs if progress is made

## 3. UI and Batch Runner Integration

- [x] 3.1 Handle `ImplUpdate::Stalled` in `app.rs` event loop — treat as failure in `advance_batch` and clear implementation state
- [x] 3.2 Add test: `advance_batch` treats stalled change as failure and skips dependents
