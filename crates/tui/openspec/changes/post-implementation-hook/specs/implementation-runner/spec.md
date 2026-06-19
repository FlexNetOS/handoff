## MODIFIED Requirements

### Requirement: Implementation loop executes Claude per task
The runner SHALL execute `claude --print --dangerously-skip-permissions` once per unfinished task. Each invocation SHALL receive a prompt that instructs Claude to:
1. Read `openspec/config.yaml` for project context and conventions
2. Read `openspec/changes/<name>/proposal.md` for change motivation and scope
3. Read `openspec/changes/<name>/design.md` for architecture decisions
4. Read `openspec/changes/<name>/specs/` for detailed requirements
5. Read `openspec/specs/` for global project specifications
6. Read `openspec/changes/<name>/tasks.md`, pick the next unfinished task, implement it, verify correctness, and mark it as completed

The prompt SHALL NOT contain hard-coded project-specific constraints (such as "Library-Constraints"). Project-specific context SHALL be provided via `openspec/config.yaml`.

After all tasks complete successfully, the implementation loop SHALL execute the post-implementation hook if configured (see post-implementation-hook spec). The `ImplUpdate::Finished` message SHALL carry a `success: bool` field indicating whether both the task implementation and the post-hook (if any) succeeded.

#### Scenario: Claude receives enriched prompt
- **WHEN** the runner invokes Claude for a task
- **THEN** the prompt instructs Claude to read config.yaml, proposal.md, design.md, change specs, and global specs before implementing
- **THEN** the prompt instructs Claude to read tasks.md, take the next unfinished task, implement it, verify correctness, and mark it completed

#### Scenario: config.yaml has project context
- **WHEN** `openspec/config.yaml` contains a `context` field with project conventions
- **THEN** Claude reads and applies those conventions during implementation

#### Scenario: config.yaml has no project context
- **WHEN** `openspec/config.yaml` has no `context` field
- **THEN** Claude continues implementation without project-specific constraints

#### Scenario: Finished message carries success flag
- **WHEN** the implementation loop completes
- **THEN** `ImplUpdate::Finished { success: bool }` SHALL be sent
- **AND** `success` SHALL be `true` only if all tasks completed AND the post-hook (if configured) succeeded

#### Scenario: Batch advancement uses success flag
- **WHEN** `advance_batch()` receives `ImplUpdate::Finished { success }`
- **THEN** it SHALL use the `success` value directly instead of re-reading task progress from disk
