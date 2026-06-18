## Context

The implementation runner in `runner.rs` spawns Claude processes in a loop to work through tasks in `tasks.md`. Currently, if Claude exits without completing any task (e.g., token exhaustion, API errors), the loop immediately aborts on non-zero exit codes, or endlessly restarts on zero exit codes with no progress. There is no mechanism to detect repeated unproductive runs and stop automatically.

## Goals / Non-Goals

**Goals:**
- Detect when the runner is stuck (no task progress after consecutive attempts)
- Automatically abort after 3 consecutive no-progress runs
- Communicate stall condition to the UI and batch runner
- Treat both error exits and successful-but-unproductive exits uniformly for stall counting

**Non-Goals:**
- Making the stall threshold configurable (hardcoded to 3 for now)
- Detecting partial progress within a single task (only completed task count matters)
- Retry strategies or automatic recovery from stalls
- Token usage monitoring or API-level error detection

## Decisions

### Decision 1: Unified stall counter for all exit types

**Choice**: Count any run that produces no new completed tasks toward the stall counter, regardless of process exit code.

**Alternatives considered**:
- *Abort immediately on non-zero exit (current behavior)*: Misses the token exhaustion case where Claude exits with code 0 but accomplishes nothing. Also too aggressive — a single transient failure kills the entire change.
- *Separate counters for errors vs. no-progress*: Added complexity with no practical benefit. From the user's perspective, "3 runs with no progress" is the relevant signal regardless of cause.

**Rationale**: A unified counter is simpler and covers all stall scenarios. The user doesn't care why progress stopped, only that it did.

### Decision 2: New `ImplUpdate::Stalled` variant

**Choice**: Add `Stalled` to `ImplUpdate` enum so the UI can distinguish stall-abort from normal completion.

**Rationale**: `Finished` currently means "all tasks done" or "process failed". Adding `Stalled` lets the UI show a specific message (e.g., "No progress after 3 attempts") and lets `advance_batch` treat it as a failure for dependency propagation.

### Decision 3: Stall counter lives in `implementation_loop`

**Choice**: Track `stall_count` and `prev_completed` as local variables inside the loop function.

**Alternatives considered**:
- *Store in `ImplState`*: Would require Arc/Mutex access from the worker thread and adds unnecessary shared state. The counter is purely internal to the loop.

**Rationale**: The stall counter is loop-internal bookkeeping. No external code needs to read or modify it.

## Risks / Trade-offs

- **[False positive stall]** A large task might take multiple Claude runs to complete (Claude handles subtasks internally). → Mitigated by counting completed *tasks* not *subtasks*. If Claude marks a task as done, the counter resets. 3 retries provides buffer for legitimate slow progress.
- **[Lost immediate-abort on error]** Removing the immediate abort on `!exited_ok` means the runner will now retry up to 3 times on errors before giving up. → This is intentional: transient failures (network, rate limits) should be retried, and 3 attempts is a reasonable limit.
