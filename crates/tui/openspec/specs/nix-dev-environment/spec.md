### Requirement: Flake provides a development shell
The project SHALL have a `flake.nix` at the repository root that defines a `devShells.default` output for the current system.

#### Scenario: Entering the dev shell
- **WHEN** a developer runs `nix develop` in the project root
- **THEN** they SHALL be placed in a shell with all required development tools available

### Requirement: Stable Rust toolchain is available
The development shell SHALL include the stable Rust toolchain from nixpkgs, including `rustc`, `cargo`, `rustfmt`, and `clippy`.

#### Scenario: Building the project
- **WHEN** a developer enters the dev shell and runs `cargo build`
- **THEN** the project SHALL compile successfully using the provided Rust toolchain

#### Scenario: Formatting code
- **WHEN** a developer enters the dev shell and runs `cargo fmt`
- **THEN** rustfmt SHALL be available and format the code

#### Scenario: Linting code
- **WHEN** a developer enters the dev shell and runs `cargo clippy`
- **THEN** clippy SHALL be available and lint the code

### Requirement: Claude Code CLI is available
The development shell SHALL include Claude Code (Anthropic's CLI tool) so developers can use AI-assisted development workflows.

#### Scenario: Running Claude Code
- **WHEN** a developer enters the dev shell and runs `claude`
- **THEN** the Claude Code CLI SHALL be available and executable

### Requirement: Flake uses standard nixpkgs input
The `flake.nix` SHALL use `nixpkgs` as its primary input for package resolution.

#### Scenario: Flake inputs
- **WHEN** inspecting `flake.nix`
- **THEN** it SHALL declare `nixpkgs` as an input
