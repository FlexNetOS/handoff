## Context

The project is a Rust TUI application using ratatui/crossterm that browses openspec changes and artifacts. It uses a Nix flake for the development environment. There is currently no README.md.

## Goals / Non-Goals

**Goals:**
- Provide a clear project description for new users
- Document how to set up the development environment (Nix flake and manual)
- Document how to build and run the application
- Explain basic usage and keyboard navigation

**Non-Goals:**
- Comprehensive user manual or tutorial
- API documentation
- Contributing guidelines (can be added later)

## Decisions

### README structure: Single file with focused sections
A single README.md at the project root with sections: description, prerequisites, build/run, usage, and license. This covers the essentials without over-documenting. The Nix flake provides the primary dev environment, but manual setup instructions should also be included for users who don't use Nix.

## Risks / Trade-offs

- **README may go stale**: As the project evolves, the README could become outdated. → Acceptable risk for a small project; keep it minimal to reduce maintenance burden.
