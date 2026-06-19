## Why

The implementation runner currently passes a minimal prompt to Claude that only references `tasks.md`. Claude starts "cold" with no awareness of the project's OpenSpec structure — no design decisions, no specs, no project context. This forces Claude to guess at architecture and conventions rather than following the documented plan, leading to lower quality implementations.

## What Changes

- Replace the minimal prompt in the implementation runner with a context-rich prompt that instructs Claude to read OpenSpec artifacts before implementing
- The prompt will reference: `openspec/config.yaml` (project context), change-level artifacts (proposal, design, specs), and global specs
- Remove the hard-coded `Library-Constraints` reference from the prompt (this belongs in `config.yaml` if needed)

## Capabilities

### New Capabilities

### Modified Capabilities
- `implementation-runner`: The Claude invocation prompt changes from a minimal task instruction to a structured prompt that includes OpenSpec context file references

## Impact

- `src/runner.rs`: The `prompt` string in `implementation_loop` is the only code change — replace the format string with the enriched prompt
