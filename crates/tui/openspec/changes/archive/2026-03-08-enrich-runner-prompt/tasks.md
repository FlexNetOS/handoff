## 1. Update Prompt String

- [x] 1.1 Replace the prompt format string in `src/runner.rs` `implementation_loop` (line 125-130) with the enriched prompt that references config.yaml, proposal.md, design.md, change specs, global specs, and tasks.md
- [x] 1.2 Remove the `(incl. Library-Constraints)` reference from the prompt

## 2. Tests

- [x] 2.1 Add a test that verifies the prompt string contains references to config.yaml, proposal.md, design.md, specs directories, and tasks.md
