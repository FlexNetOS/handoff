## ADDED Requirements

### Requirement: Per-change configuration file
The system SHALL store per-change configuration in a file named `change-config.yaml` inside the change directory. The file SHALL contain a `depends_on` list (default empty) and a `run_mode` field (default `"normal"`).

#### Scenario: Read config with all fields
- **WHEN** a change directory contains `change-config.yaml` with `depends_on` and `run_mode` fields
- **THEN** the system SHALL parse both fields and return the complete configuration

#### Scenario: Read config with only dependencies
- **WHEN** a change directory contains `change-config.yaml` with only a `depends_on` field
- **THEN** the system SHALL return the dependencies and default `run_mode` to `"normal"`

#### Scenario: Read config with only run_mode
- **WHEN** a change directory contains `change-config.yaml` with only `run_mode: apply`
- **THEN** the system SHALL return an empty dependency list and `run_mode` as `"apply"`

#### Scenario: No config file exists
- **WHEN** a change directory has no `change-config.yaml` file
- **THEN** the system SHALL return an empty dependency list and `run_mode` as `"normal"`

### Requirement: Write per-change configuration
The system SHALL write the complete `ChangeConfig` to `change-config.yaml` when dependencies or run mode are modified.

#### Scenario: Write config preserves all fields
- **WHEN** the system writes a `ChangeConfig` with dependencies and `run_mode: apply`
- **THEN** `change-config.yaml` SHALL contain both `depends_on` and `run_mode` fields

### Requirement: Toggle run mode in dependency view
The user SHALL be able to toggle the run mode by pressing `M` in the dependency view. The current run mode SHALL be displayed in the view.

#### Scenario: Toggle from normal to apply
- **WHEN** the user presses `M` in the dependency view and the current mode is `normal`
- **THEN** the run mode SHALL change to `apply` and persist to `change-config.yaml`

#### Scenario: Toggle from apply to normal
- **WHEN** the user presses `M` in the dependency view and the current mode is `apply`
- **THEN** the run mode SHALL change to `normal` and persist to `change-config.yaml`

#### Scenario: Run mode displayed in view
- **WHEN** the dependency view is shown for a change
- **THEN** the current run mode SHALL be displayed (e.g., "Mode: normal" or "Mode: apply")
