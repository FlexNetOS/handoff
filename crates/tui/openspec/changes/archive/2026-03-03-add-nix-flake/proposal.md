## Why

This Rust project currently has no reproducible development environment setup. Developers need to manually install the correct Rust toolchain and development tools. A Nix flake will provide a declarative, reproducible development shell with stable Rust and Claude Code available out of the box.

## What Changes

- Add a `flake.nix` to the project root providing a development shell
- The dev shell includes stable Rust (via nixpkgs or rust-overlay) for building the project
- The dev shell includes Claude Code (Anthropic's CLI tool) for AI-assisted development
- Add a `.gitignore` entry for Nix-related generated files if needed

## Capabilities

### New Capabilities
- `nix-dev-environment`: Nix flake providing a reproducible development shell with stable Rust toolchain and Claude Code CLI

### Modified Capabilities

## Impact

- New files: `flake.nix`, `flake.lock` at project root
- Dependencies: Requires Nix with flakes enabled on the developer's machine
- No impact on existing Rust source code or build configuration
