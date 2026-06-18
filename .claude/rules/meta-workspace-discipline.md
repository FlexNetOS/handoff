# Workspace Discipline

You are working in the **handoff** workspace — a Rust workspace with the `hf` CLI, `ledger`, and `work-order` crates.

## Required Behaviors

1. **Use `hf` for continuity operations** — NOT raw ad-hoc edits
   - `hf status` shows task state
   - `hf claim <id>` reserves a task
   - `hf checkpoint <id>` witnesses progress
   - `hf done <id>` marks completion
   - `hf handoff` renders the next-session packet

2. **Run tests and drift before finishing**
   - `cargo test --workspace`
   - `cargo clippy --workspace --all-targets -- -D warnings`
   - `hf drift`

3. **Check scope before committing**
   - `git status` to see changed files
   - Verify they match the claimed task's path scope
   - Use `hf ship <id>` or branch + PR for promotion

4. **Target precisely with git**
   - Stage only files in the task scope
   - Never commit generated artifacts (ledger.db, packets, active.md, target/)
