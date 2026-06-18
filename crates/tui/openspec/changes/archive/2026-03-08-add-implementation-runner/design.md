## Context

The openspec-tui is a synchronous TUI built with ratatui/crossterm. It has three screens (ChangeList, ArtifactMenu, ArtifactView) managed via a screen stack. All operations are currently blocking. The user has a working shell script (`ralph-implement.sh`) that loops through tasks calling `claude --print --dangerously-skip-permissions`, and wants this integrated into the TUI with a progress bar.

The main event loop in `main.rs` uses `crossterm::event::read()` which blocks until a key event arrives. The `App` struct holds screen state and handles input.

## Goals / Non-Goals

**Goals:**
- Run Claude implementation loop in a background thread while TUI stays responsive
- Show a persistent progress bar at the bottom of all screens during implementation
- Allow starting implementation with `R` from the ArtifactMenu
- Allow stopping implementation with `S` from any screen
- Redirect Claude output to a log file
- Track progress by re-reading tasks.md after each Claude iteration

**Non-Goals:**
- Showing Claude's live output in the TUI (too complex, captured to log file instead)
- Running multiple implementations in parallel
- Configuring the Claude command or prompt from the TUI
- Supporting non-Claude implementation tools

## Decisions

### 1. Threading model: std::thread + mpsc channel

Use `std::thread::spawn` for the worker and `std::sync::mpsc::channel` for communication back to the TUI thread.

**Why over async:** The TUI is synchronous. Adding tokio/async-std for a single background process would be over-engineering. std::thread is zero-dependency and sufficient.

**Why over subprocess-only:** We need the TUI to remain responsive during implementation. A blocking subprocess call would freeze the UI.

### 2. Event loop: switch from blocking to polling

Change `crossterm::event::read()` to `crossterm::event::poll(Duration)` + `read()`. This lets the main loop check the mpsc channel for worker updates between key events.

**Poll interval:** 500ms is sufficient — progress changes only when a full Claude iteration completes (which takes seconds to minutes). This keeps CPU usage negligible.

**Alternative considered:** Using a separate thread to forward crossterm events through the channel. Rejected as unnecessarily complex — polling with timeout is the standard ratatui pattern for background work.

### 3. Progress tracking: re-read tasks.md

After each Claude iteration, the worker reads `tasks.md` and counts checked (`- [x]`) vs unchecked (`- [ ]`) boxes, then sends the counts over the channel.

**Why not parse Claude output:** Claude's output format is unpredictable. The tasks.md file is the source of truth — Claude writes to it, the TUI reads from it.

### 4. Status bar: conditional layout split

When `app.implementation.is_some()`, the `draw()` function splits the terminal area into two chunks: main content (all remaining space) and a 2-line status bar at the bottom.

When no implementation is running, the full area is used for content as before.

```
┌─────────────────────────────────┐
│                                 │
│   Normal screen content         │  ← Layout::vertical
│   (ChangeList/ArtifactMenu/     │     Constraint::Min(0)
│    ArtifactView)                │
│                                 │
├─────────────────────────────────┤
│ ⟳ change-name  3/7  ████░░ 42% │  ← Constraint::Length(2)
│ [S] Stop                        │
└─────────────────────────────────┘
```

### 5. Worker cancellation: shared AtomicBool

Use `Arc<AtomicBool>` as a cancellation flag. The worker checks it before each iteration. When the user presses `S`, the main thread sets the flag and kills the active Claude child process.

**Why AtomicBool over channel:** Simpler for a single boolean signal. The worker only needs to check "should I stop?" — no complex messages needed.

**Child process cleanup:** The worker holds the `Child` handle in an `Arc<Mutex<Option<Child>>>` shared with the main thread. On cancellation, the main thread can kill the active process immediately rather than waiting for it to complete.

### 6. Claude command construction

Reuse the cross-platform command pattern from `data.rs` (cmd wrapper on Windows). The Claude command and prompt mirror the shell script:

```
claude --print --dangerously-skip-permissions "Read openspec/changes/<name>/tasks.md, take the next unfinished task, implement this task, verify if the changes are correct (incl. Library-Constraints), and mark the task as completed."
```

### 7. Log file location

Write Claude output to a temp file: `/tmp/openspec-implement-<change-name>.log` (or `std::env::temp_dir()` for cross-platform). Each iteration appends to the file. The log file path is shown in the status bar so the user can inspect it.

## Risks / Trade-offs

**[Claude modifies files while user browses]** → The TUI only reads files for display, so there's no conflict. If the user views tasks.md during implementation, it may show stale content until they re-enter the view. Acceptable for v1.

**[Claude process hangs indefinitely]** → No timeout implemented in v1. The user can press `S` to kill the process. A timeout could be added later if needed.

**[tasks.md format unexpected]** → If tasks.md doesn't have standard checkbox format, progress counting will show 0/0. The implementation loop (grep for `- [ ]`) will simply exit. No crash risk.

**[Multiple `R` presses]** → Ignore `R` if `implementation.is_some()`. Only one implementation can run at a time.
