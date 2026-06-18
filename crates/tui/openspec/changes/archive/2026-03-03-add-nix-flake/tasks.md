## 1. Create Flake

- [x] 1.1 Create `flake.nix` at project root with `nixpkgs` input and `devShells.default` output
- [x] 1.2 Add stable Rust toolchain packages (`cargo`, `rustc`, `rustfmt`, `clippy`) to the dev shell
- [x] 1.3 Add Claude Code to the dev shell (via `claude-code` package or nodejs fallback)
- [x] 1.4 Generate `flake.lock` by running `nix flake lock`

## 2. Verification

- [x] 2.1 Verify `nix develop` enters a shell with `cargo`, `rustc`, `rustfmt`, `clippy` available
- [x] 2.2 Verify `claude` CLI is available in the dev shell
- [x] 2.3 Verify `cargo build` compiles the project successfully inside the dev shell
