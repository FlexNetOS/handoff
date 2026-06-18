## Context

The implementation runner in `src/runner.rs` spawns Claude in a loop to implement tasks from a change's `tasks.md`. Currently, the prompt passed to Claude is minimal — it only tells Claude to read `tasks.md` and implement the next task. Claude has no awareness of the project's OpenSpec structure, design decisions, or specifications.

The existing prompt (in `implementation_loop`, line 125-130):
```
Read openspec/changes/{name}/tasks.md, take the next unfinished task,
implement this task, verify if the changes are correct (incl. Library-Constraints),
and mark the task as completed.
```

## Goals / Non-Goals

**Goals:**
- Give Claude structured context about the OpenSpec project before it starts implementing
- Reference config.yaml, change artifacts (proposal, design, specs), and global specs
- Remove the hard-coded `Library-Constraints` reference
- Keep the prompt as file-path references (Option A), not embedded content

**Non-Goals:**
- Embedding file contents directly into the prompt
- Changing how Claude is invoked (flags, command structure)
- Adding new dependencies or modules

## Decisions

### 1. Prompt structure: file-path references (Option A)

Instruct Claude to read specific files rather than embedding their contents into the prompt.

**Why:** The prompt stays short and stable regardless of how large the specs grow. Claude reads files as its first step, which it already does well. This also means the prompt doesn't need to change when artifact content changes.

**Alternative considered:** Embedding file contents (Option B) — rejected because it makes the prompt unpredictably long and duplicates what Claude can read itself.

### 2. Reference order: broad to narrow

The prompt lists files from broadest context to most specific:
1. `openspec/config.yaml` — project-level context
2. `openspec/changes/{name}/proposal.md` — change motivation
3. `openspec/changes/{name}/design.md` — architecture decisions
4. `openspec/changes/{name}/specs/` — detailed requirements
5. `openspec/specs/` — global project specs
6. `openspec/changes/{name}/tasks.md` — the task list

This gives Claude a natural context funnel before it starts implementing.

### 3. Remove Library-Constraints from prompt

The old prompt had `(incl. Library-Constraints)` hard-coded. This is project-specific and belongs in `openspec/config.yaml` under the `context` field. Removing it makes the runner generic across projects.

## Risks / Trade-offs

**[Claude may not read all files]** → The prompt explicitly instructs Claude to read them "before implementing". This is a strong enough signal for Claude's instruction-following. If a file doesn't exist (e.g., no global specs yet), Claude will note it and continue.

**[Longer initial phase per iteration]** → Claude now reads several files before starting work. This adds seconds per iteration but significantly improves implementation quality. Acceptable trade-off.
