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
