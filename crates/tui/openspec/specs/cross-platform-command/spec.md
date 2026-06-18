## MODIFIED Requirements

### Requirement: Platform-aware openspec command construction
The system SHALL provide a function that constructs a `std::process::Command` for invoking `openspec`, using the correct invocation method for the current platform.

#### Scenario: Unix/macOS invocation
- **WHEN** the application runs on a Unix or macOS platform
- **THEN** the command SHALL be constructed as `Command::new("openspec")` (direct binary resolution via PATH)

#### Scenario: Windows invocation
- **WHEN** the application runs on Windows
- **THEN** the command SHALL be constructed as `cmd.exe /C openspec` to ensure `.cmd` wrapper files are resolved via PATHEXT

### Requirement: All openspec CLI calls use the shared command constructor
All functions that invoke the `openspec` CLI SHALL use the shared command constructor instead of calling `Command::new("openspec")` directly.

#### Scenario: list_changes uses shared constructor
- **WHEN** `list_changes()` invokes the openspec CLI
- **THEN** it SHALL use the shared `openspec_command()` function

#### Scenario: get_change_status uses shared constructor
- **WHEN** `get_change_status()` invokes the openspec CLI
- **THEN** it SHALL use the shared `openspec_command()` function

## REMOVED Requirements

### Requirement: Platform-aware claude command construction
The `claude_command()` function that constructs a hardcoded `Command::new("claude")` is removed. The runner now constructs commands from the configurable command template instead.

**Reason**: Replaced by the config-driven command template system in the `tui-configuration` capability.
**Migration**: The runner uses `TuiConfig.command` with whitespace splitting instead of `claude_command()`.
