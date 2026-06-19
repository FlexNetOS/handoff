## ADDED Requirements

### Requirement: Project README with essential information
The project SHALL have a README.md at the repository root that describes the project, its prerequisites, and how to build and run it.

#### Scenario: README contains project description
- **WHEN** a user opens the README.md
- **THEN** they find a description of what openspec-tui is (a terminal UI for browsing openspec changes and artifacts)

#### Scenario: README contains prerequisites
- **WHEN** a user reads the prerequisites section
- **THEN** they find the required dependencies listed (Rust toolchain, openspec CLI)

#### Scenario: README contains build and run instructions
- **WHEN** a user reads the build/run section
- **THEN** they find instructions for both Nix flake setup and manual cargo build/run

#### Scenario: README contains usage overview
- **WHEN** a user reads the usage section
- **THEN** they find the keyboard navigation keys (arrow keys, j/k, Enter, Esc, q) and the screen flow (Change List, Artifact Menu, Artifact View)
