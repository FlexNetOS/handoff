# ADR-0013 — Adopt FlexNetOS meta conventions in handoff

**Status:** accepted (2026-06-18) · **Owner:** handoff kernel · **Derived from:** HFTASK-0016, META-ORG-POLICY.md, rusty-idd drift reports (R12), ADR-0001 §9.6.

## Context

`handoff` is a member of the FlexNetOS meta workspace but lacked the org convention set used by sibling repos. Three rusty-idd-vs-meta drift reports (spot-verified in `~/Desktop/meta`) showed that missing conventions cause friction: non-semantic merge subjects, ad-hoc release mechanics, Dependabot/Justfile/python-pre-commit drift, and unguarded destructive commands. Adopting the same conventions keeps the continuity kernel aligned with the fleet and lets the loop promote changes through the same gates as the rest of the org.

## Decision

1. **Conventional Commits + semantic PR titles**: add `commitlint.config.cjs` (12 types) and `.github/workflows/semantic-pr-title.yml` as the merge-blocking guard. Local `.githooks/commit-msg` validates subjects before push when installed via `make install-hooks`.
2. **Makefile (not Justfile)**: add `Makefile` with `build`, `test`, `fmt`, `fmt-check`, `clippy`, `release`, `clean`, and `install-hooks` targets. This matches the meta convention (D7) and the pre-existing local push hook expectations.
3. **Git hooks (not python pre-commit)**: add `.githooks/{commit-msg,pre-commit,pre-push}`. `pre-commit` runs `cargo fmt --check`; `pre-push` runs fmt + clippy + tests. `make install-hooks` symlinks them into the active git hooks path.
4. **Renovate (not Dependabot)**: add `renovate.json` extending `config:recommended` (D3).
5. **Release Please (not cargo-dist)**: add `release-please-config.json`, `.release-please-manifest.json`, and `VERSION` (simple release type, 5-platform build handled by CI, no crates.io/homebrew automation for this repo yet).
6. **CI matrix + pinned toolchain**: update `.github/workflows/ci.yml` to use Rust `1.96.0` and a 3-OS matrix (ubuntu, macos, windows) with `Swatinem/rust-cache@v2`. Add `.github/workflows/release.yml` for 5-platform release artifacts on tags.
7. **Promote-verify gate**: add `.github/workflows/promote-verify.yml` implementing the rusty-idd two-tier develop→master gate: clean-merge probe, locked build/test, fmt/clippy, `hf drift`, and `cargo audit --deny warnings`.
8. **Agent guard + rules**: add `.claude/agent-guard.toml`, `.claude/rules/*.md`, and update `.claude/settings.json` with a `statusLine` and `permissions`. The destructive-command and handoff-markdown file-pattern guards protect against common mistakes.
9. **Contributing guide**: add `CONTRIBUTING.md` documenting Conventional Commits and local hook installation.
10. **Workspace version**: add `version = "0.1.0"` to `[workspace.package]` so release-please can keep `Cargo.toml` and `VERSION` in sync.

## Consequences

- PR titles and merge subjects must follow the 12-type enum. The semantic PR title check blocks merges that do not.
- Local pushes run the same fast gates as CI when hooks are installed.
- Releases are driven by release-please tags; manual release creation is no longer needed.
- The Windows CI matrix may expose path-dependency or shell assumptions; fixes should be treated as CI defects, not ignored.
- The agent-guard file-pattern rule enforces ADR-0004 §1: no new handoff-style markdown outside `.handoff/`.
