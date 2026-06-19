## Context

The TUI hardcodes the Claude Code invocation (`claude --print --dangerously-skip-permissions {prompt}`) in `runner.rs` and the implementation prompt in `build_prompt()`. The command constructor in `data.rs` (`claude_command()`) is also hardcoded. Users who want to use alternative AI CLI tools or customize the prompt must modify source code.

The app currently has three screens (`ChangeList`, `ArtifactMenu`, `ArtifactView`) using a stack-based navigation model with push/pop via Enter/Esc.

## Goals / Non-Goals

**Goals:**
- Configurable command template with `{prompt}` placeholder
- Configurable prompt template with `{name}` placeholder
- YAML config file at `openspec/tui-config.yaml` (project-local)
- Dedicated Config screen accessible via `C` from any screen
- Inline text editing for command field, `$EDITOR` for prompt field
- Current hardcoded values as defaults when no config file exists
- Reset to defaults writes default values explicitly into the file

**Non-Goals:**
- Global/user-level config (only project-local for now)
- Predefined tool profiles/templates (future enhancement)
- Shell-based command execution (no `sh -c`, just whitespace splitting)
- Configuring the `openspec` CLI command itself

## Decisions

### Config file format: YAML at `openspec/tui-config.yaml`
Lives alongside `openspec/config.yaml`. YAML chosen because the project already uses `serde` and adding `serde_yaml` is minimal. The file has two fields: `command` and `prompt`.

**Alternative considered:** TOML — equally viable but YAML is consistent with OpenSpec's own config format.

### Command parsing: Whitespace split with `{prompt}` as single argument
The command template string (e.g., `claude --print --dangerously-skip-permissions {prompt}`) is split on whitespace. The token containing `{prompt}` is replaced with the rendered prompt as a single argument (not shell-expanded). The first token becomes the binary, the rest become args.

**Alternative considered:** Shell execution via `sh -c` — more flexible but introduces escaping issues and platform differences. Structured `command` + `args` fields — safer but more verbose config.

### Config screen: New `Screen::Config` variant
Pushed onto `screen_stack` like other screens. Two fields rendered:
- Command: inline single-line text input with cursor
- Prompt: read-only preview (first few lines) with Enter to open `$EDITOR` (falls back to `vi`)

Keybindings: `S` to save, `Esc` to cancel (discard changes), `D` to reset to defaults. Tab to switch between fields.

### Config state: `TuiConfig` struct in `App`
Loaded once at startup. The Config screen works on a clone; on save, it replaces the App's config and writes to disk. On cancel, the clone is discarded.

### Runner integration: Config replaces hardcoded values
`build_prompt()` reads `config.prompt` and substitutes `{name}`. The runner reads `config.command`, substitutes `{prompt}`, splits into binary + args, and spawns. The existing `claude_command()` in `data.rs` is no longer used by the runner (but remains for the `openspec` CLI).

## Risks / Trade-offs

- **Whitespace splitting limits flexibility** → Acceptable for v1. Users needing pipes/redirects can wrap in a shell script and point the command at it.
- **No validation of command template** → If `{prompt}` is missing, the prompt is silently lost. Mitigation: warn in Config screen if `{prompt}` not found in command.
- **`$EDITOR` suspends the TUI** → Standard behavior for terminal apps (e.g., git commit). The TUI restores on editor exit. If `$EDITOR` is unset, fall back to `vi`.
- **Config file in project dir could be committed** → User's choice. Could add to `.gitignore` but that's a project decision, not ours.
