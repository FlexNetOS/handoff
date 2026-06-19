## Context

The openspec-tui project is a Rust application (edition 2024) with no current environment management. Developers must manually ensure they have the correct Rust toolchain and tools installed. The project needs a reproducible dev environment via Nix flakes.

## Goals / Non-Goals

**Goals:**
- Provide a single `nix develop` command to enter a fully configured dev shell
- Include stable Rust toolchain (rustc, cargo, rustfmt, clippy) from nixpkgs
- Include Claude Code CLI for AI-assisted development
- Keep the flake minimal and easy to maintain

**Non-Goals:**
- Cross-compilation or multi-target builds
- CI/CD integration (can be added later)
- Nix-based build of the project itself (cargo remains the build tool)
- Pinning a specific Rust version via rust-overlay (nixpkgs stable is sufficient)

## Decisions

### Use nixpkgs stable Rust instead of rust-overlay
**Decision**: Use the Rust toolchain from nixpkgs directly rather than adding fenix or rust-overlay as an extra input.
**Rationale**: The project uses `edition = "2024"` which is supported by recent nixpkgs Rust. Adding rust-overlay increases flake complexity for no benefit here. If a specific Rust version is needed later, rust-overlay can be added.

### Claude Code via nixpkgs or unfree package
**Decision**: Include `claude-code` from nixpkgs. If not available, use a direct npm/node-based approach or an overlay.
**Rationale**: Claude Code is distributed as an npm package. The simplest approach is to include nodejs and install it, or use a nixpkgs package if one exists.

### Flake structure
**Decision**: Single `flake.nix` at project root with one `devShells.default` output.
**Rationale**: The project has a single development workflow. No need for multiple shells or packages outputs.

## Risks / Trade-offs

- **[Claude Code packaging]** → Claude Code may not be in nixpkgs yet. Mitigation: Fall back to including `nodejs` and `nodePackages.npm` so developers can `npm install -g @anthropic-ai/claude-code`, or use a custom derivation.
- **[Nixpkgs Rust version lag]** → nixpkgs stable Rust may lag behind the latest release. Mitigation: Acceptable for this project; can switch to rust-overlay if needed later.
- **[Flake lock updates]** → `flake.lock` must be updated periodically. Mitigation: Run `nix flake update` as needed.
