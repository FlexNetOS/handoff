{
  # Dev shell for the rusty-idd Cargo workspace (crates/{core,runner,tui,spec,cli}
  # → the single `rusty-idd` binary). Lives here for historical reasons (it was
  # the openspec-tui dev shell, retired in slice 8) and is the documented nix
  # entrypoint per CLAUDE.md; it builds/runs the whole workspace from the repo root.
  description = "rusty-idd development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  };

  outputs = { self, nixpkgs }:
    let
      supportedSystems = [ "x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin" ];
      forAllSystems = nixpkgs.lib.genAttrs supportedSystems;
      # The workspace MSRV floor (measured in slice A4): edition 2024 + `let`-chains
      # (stabilized 1.88) + time 0.3.47 (>=1.88) + ratatui 0.30 (>=1.86). Keep this
      # in lockstep with the per-crate `rust-version = "1.88"` and CI's `msrv` job.
      msrvFloor = "1.88.0";
    in
    {
      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            config.allowUnfree = true;
          };
          # Hard floor guard: fail `nix develop` evaluation if the pinned nixpkgs
          # ever provides a rustc below the workspace MSRV, so the dev shell can
          # never drift below what the crates require. flake.lock pins the exact
          # nixpkgs revision (reproducibility); this assertion enforces the floor.
          rustcOk = pkgs.lib.assertMsg
            (builtins.compareVersions pkgs.rustc.version msrvFloor >= 0)
            "rusty-idd MSRV floor is ${msrvFloor} but nixpkgs provides rustc ${pkgs.rustc.version}; bump the pinned nixpkgs input.";
        in
        {
          default = assert rustcOk; pkgs.mkShell {
            # Rust toolchain to build/test/lint the workspace, plus claude-code
            # (the agent CLI that `rusty-idd tui` / `rusty-idd run` drives).
            # NOTE: the Node OpenSpec CLI is no longer an input — the lifecycle
            # engine is native Rust (crates/spec). The Node CLI is only a
            # dev-time conformance oracle, invoked ad hoc via `bunx`/`npx`.
            buildInputs = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.claude-code
            ];
            shellHook = ''
              echo "rusty-idd dev shell — run from the workspace root:"
              echo "  cargo build --workspace          # build the rusty-idd binary"
              echo "  cargo run --bin rusty-idd -- --help"
              echo "  cargo run --bin rusty-idd -- tui # the former openspec-tui"
            '';
          };
        });
    };
}
