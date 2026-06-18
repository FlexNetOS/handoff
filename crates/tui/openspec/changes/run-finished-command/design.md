## Context

Users start implementation runs that can take minutes to hours. They switch to other work and have no way to know when the run finishes. The existing `post_implementation_prompt` is an AI-prompt hook that runs inside the runner thread — it only fires in Normal mode on success, and it's designed for chaining AI work, not for user notifications.

We need a lightweight shell command that fires once on the TUI side when all work is done, regardless of outcome.

## Goals / Non-Goals

**Goals:**
- Add a `run_finished_command` config field that holds an arbitrary shell command
- Fire it exactly once when the entire run (single or batch) is done
- Fire regardless of success, failure, or stall
- Fire-and-forget: don't block the TUI, don't capture output, don't care about exit code
- Editable from the Config screen like other fields

**Non-Goals:**
- No placeholder substitution (`{name}`, `{status}`) — keep it simple for now
- No per-change notification commands — this is a global TUI config
- No output capture or error reporting for the notification command
- No changes to the runner thread — this lives purely on the TUI/App side

## Decisions

### Decision 1: Fire on TUI side, not in runner thread

The notification fires in `poll_implementation()` when the implementation is cleared and no batch successor is started. This is the single point where the TUI knows "everything is done."

**Rationale**: The runner thread already has the post-implementation-prompt for in-runner hooks. The notification is a UI-level concern ("tell the user"). Placing it in `poll_implementation` means it naturally works for both single runs, apply-mode runs, and batch runs without any runner changes.

**Alternatives considered**: Firing inside the runner thread after `Finished` — rejected because batch runs would fire per-change, not once at the end.

### Decision 2: Fire-and-forget with `std::process::Command::spawn()`

Spawn the command and immediately drop the `Child` handle. No waiting, no output capture.

**Rationale**: This is a notification command. The user doesn't want the TUI to hang while `curl` sends a push notification. If the command fails, that's a config problem the user can debug independently.

### Decision 3: Use `sh -c` for shell command execution

Instead of splitting on whitespace, pass the entire string to `sh -c "<command>"`. This allows pipes, redirects, and complex expressions like `ntfy pub mytopic "Done!" && notify-send "Run finished"`.

**Rationale**: Notification commands often involve pipes or quoting (e.g., `curl -d "message=done" https://...`). Whitespace splitting would break these. Using `sh -c` is the standard approach for user-provided shell commands.

**Cross-platform**: On Windows, use `cmd /C` instead of `sh -c`, matching the existing cross-platform pattern in `data.rs`.

### Decision 4: Inline-editable in Config screen (same as Command field)

The `run_finished_command` field uses inline editing (Enter to edit, character-by-character), like the `command` and `interactive_command` fields. It's a short one-liner, not a multiline prompt.

**Rationale**: Notification commands are typically short (`notify-send "Done"`, `ntfy pub topic msg`). Opening `$EDITOR` for this would be overkill.

### Decision 5: Place in Tab cycle between InteractiveCommand and Command

Tab order: Command → Prompt → PostImplementationPrompt → InteractiveCommand → RunFinishedCommand → Command.

**Rationale**: Grouping it after InteractiveCommand keeps the "shell commands" (Command, InteractiveCommand, RunFinishedCommand) conceptually near each other, with the "prompt" fields in between.

## Risks / Trade-offs

- **[Zombie processes]** → Fire-and-forget means the child process is not waited on. On Unix this can create zombies. Mitigation: the process will be reaped when the TUI exits. For short-lived notification commands this is acceptable.
- **[Shell injection]** → The user provides the command themselves via their own config file. This is intentional — the same trust model as `command` and `interactive_command`. No mitigation needed.
- **[No feedback on failure]** → If the notification command fails, the user gets no indication in the TUI. This is acceptable for v1 — the user can test their command independently.
