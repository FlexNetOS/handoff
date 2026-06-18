## Why

The TUI currently hardcodes the Claude Code command (`claude --print --dangerously-skip-permissions`) and the implementation prompt in `runner.rs`. Users cannot switch to alternative CLI tools (e.g., aider, codex, custom scripts) or customize the prompt without modifying source code. Adding a configuration system makes the TUI tool-agnostic and user-adaptable.

## What Changes

- Add a YAML configuration file (`openspec/tui-config.yaml`) with command template and prompt template fields
- Add a new Configuration screen accessible from any screen via `C` key
- Inline editing for single-line fields (command), `$EDITOR` for multi-line fields (prompt)
- Support `{prompt}` placeholder in command template and `{name}` placeholder in prompt template
- Default values match current hardcoded behavior when no config file exists
- Runner reads config instead of using hardcoded values

## Capabilities

### New Capabilities
- `tui-configuration`: Configuration file loading, saving, defaults, and placeholder substitution for command and prompt templates
- `config-screen`: TUI screen for viewing and editing configuration with inline text input and external editor support

### Modified Capabilities
- `cross-platform-command`: The command construction in `data.rs` will be replaced by config-driven command template parsing

## Impact

- `runner.rs`: `build_prompt()` and command construction replaced by config-driven logic
- `data.rs`: `claude_command()` replaced or adapted to use config
- `app.rs`: New `Screen::Config` variant, new input handler, config state in `App`
- `ui.rs`: New `draw_config_screen()` function
- New dependency: `serde_yaml` (or reuse existing serde with yaml feature)
