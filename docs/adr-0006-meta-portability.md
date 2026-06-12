# ADR-0006 — meta portability: total internalization, envctl as the box materializer

**Status:** accepted (2026-06-12) · **Owner:** envctl (box layer) + meta parent (registration/bootstrap) ·
**Derived from:** user directives 2026-06-12 ("all work on this system is now meta work; nothing should be
outside meta; IDE tools exempt" + "settings linked from user-global INTO meta" + "always research and verify
manually first after ICM memory recall"), PORTABILITY-SESSION-PROMPT.md, PORTABILITY-AUDIT.md (phase-1
matrix), open-questions r4 item 16.

## Context

The meta workspace (~64 repos) must be **clone-and-bootstrap portable**: on a fresh machine, cloning
`FlexNetOS/meta` and running one entrypoint must materialize the full environment. The phase-1 audit
(PORTABILITY-AUDIT.md, evidence in `/tmp/portability-sweep.out`) found the correct pattern — real file
in meta, symlink outside — in exactly **3 places** (`lane`, `n8n-up/down`, `statusline-command.sh`),
against ~20 binary *copies* in `~/.local/bin`/`~/.cargo/bin`/`/usr/local/bin` (vox exists 3×, kst/kasetto
2×, weave's cargo copy is the known-stale one), real global files in `~/.claude`, an unregistered
yazelix layer (nix profile `github:luccahuguet/yazelix` @ `e60d15e` + user config in `~/.config/yazelix`),
3 user systemd units (one — repowire — crash-looping on a missing binary), and work dirs outside meta
(`~/Downloads/tmp/*`, `~/Desktop/workspaces/Devin`).

Two tools that already live in meta claim this exact territory:
- **envctl** (README): "manages the box declaratively — every tool is a TOML component whose lifecycle
  hooks wrap the proven bash from the Desktop kit". Verbs: `auto-detect / install (idempotent, dep-ordered)
  / auto-fix / reset / add-repo / graph / lock --check / doctor`. It already owns the **whole yazelix
  stack** (manifest/nix-yazelix.toml: nix → cachix → home-manager → yazelix → desktop → shell auto-enter →
  config) with declarative reversible **wiring kinds** (`shell_rc` marker blocks, `alternatives`,
  `nix_conf_lines`), ships the Desktop kit in `assets/scripts/` (the `/usr/local/bin` and `~/Desktop`
  copies are deployed artifacts of in-repo files), and has `manifest/components.d/` for drop-ins plus a
  content-hashed `envctl.lock` CI gate.
- **kasetto** (`~/.config/kasetto/kasetto.yaml`, live): "this is the agent layer that applies everywhere…
  **The OS/toolchain layer is envctl**" — kasetto manages skills/MCP/commands into `.claude`/`.codex`,
  and envctl already wires `kasetto sync --locked` in as a component (manifest/agent-env.toml).

## Decision

1. **envctl is the single box materializer** (adopt-then-extend; S1/S2 lock). No GNU stow (foreign,
   non-Rust, no detect/verify/lock model), no new `meta env link` subcommand (would duplicate envctl's
   engine), no kasetto overreach (it is the agent layer by its own definition). The portability gap is
   **component coverage + config residency**, not a missing engine.
2. **`envctl/home/` becomes the canonical home tree** for user-global, non-secret configs: bashrc
   snippet(s), `~/.profile`/`.zshrc` snippets, systemd user units, ghostty, nushell (`config.nu`,
   `rtk-wrappers.nu`), `.gitconfig`, global `kasetto.yaml`, `~/.claude/{settings.json, CLAUDE.md, RTK.md,
   commands/}`, `~/.config/yazelix/*` user config. Each is wired by an envctl component that **archives
   the original to `~/Desktop/_archives/<name>-<date>/` then symlinks** the home path into the repo tree.
   Interim wiring = script-kind components in `manifest/components.d/` (the wrap-proven-bash pattern);
   the engine-native `wiring.symlink` kind is a follow-up envctl feature (via its Feature Forge harness).
   Tool repos stay free of personal config (rtk's repo gets no personal filters.toml; the home tree owns it).
3. **Binary canon = symlink → `~/Desktop/meta/<repo>/target/release/<bin>`** (the lane pattern, fixed to
   *release*; lane itself currently points at debug and gets fixed; `~/.claude/settings.json` weave hooks
   move from `target/debug/weave` to release in the same stroke). Copies in `~/.local/bin` and duplicate
   generations in `~/.cargo/bin` (kasetto, kst, weave, grit, secretctl, secretd) become links; root-owned
   `/usr/local/bin` copies (archon, vox) get a sudo wiring phase. **Caveat recorded:** `cargo install`
   of a meta member would overwrite its link — the envctl component builds from the meta workspace
   instead, and `verify` asserts link-ness, so drift is caught by `doctor`.
4. **yazelix is homed**: genuine FlexNetOS fork (fork lessons apply: auto-suffix + silent success →
   always re-query), registered in `.meta.yaml` (Tier C fork) + cloned at `meta/yazelix`; the envctl
   yazelix component's install URL flips `github:luccahuguet/yazelix` → `github:FlexNetOS/yazelix`; a
   pin branch records the currently-installed drift point `e60d15e` (upstream-pin policy template).
   `~/.config/yazelix` user files move to `envctl/home/.config/yazelix/` + symlink (replacing the
   install-if-missing generation of `yazelix-config.sh`); `~/.local/share/yazelix` is yazelix-owned
   runtime state and stays machine-local (upstream's own docs call it generated output).
5. **Secrets and state never internalize.** `~/.claude/.credentials.json`, `~/.config/gh/hosts.yml`,
   keyrings, env-ctl vault material = secret-never (envctl's secrets stack / relay is the sanctioned
   channel — forgotten-directive #1). Histories, caches, sessions, `vox.db`, piper voices, ollama
   models, `~/.local/share/*` tool state = state-stays (bootstrap re-downloads/regenerates).
6. **systemd user units** move to `envctl/home/.config/systemd/user/` + links; `ExecStart` paths point
   at meta-linked binaries; the stale `Documentation=~/Desktop/envctl` path is fixed. **repowire**: unit
   is enabled + crash-looping (`Restart=always`, binary absent from `~/.local/bin`, 268MB daemon.log) —
   disable now (reversible: `systemctl --user enable --now repowire.service`), owner decision tracked in
   a KB incident; the log file is left for the human.
7. **Work dirs**: `~/Downloads/tmp/handoff` is **blocked on the forgotten-directive cross-reference**
   (lite-version check vs `meta/handoff`) — never moved blindly; the rest of `~/Downloads/tmp` and the
   Desktop strays relocate into meta or `~/Desktop/_archives/` per the audit, lowest priority.
   `~/Desktop/_archives/` itself stays machine-local **by policy** (it is the safety net).
8. **Bootstrap entrypoint = `meta/scripts/bootstrap.sh`**, a thin sequencer over the real engines:
   (0) rustup, (1) clone + `meta git update`, (2) `cargo build --release` the tool crates,
   (3) `envctl install` (nix/yazelix/links/units; sudo phases flagged), (4) `kasetto sync --locked`
   (respecting the live kasetto.yaml safety note: global sync must be dry-run-verified to be additive),
   (5) `envctl doctor` + `envctl lock --check` as the green gate. `FlexNetOS/agent-skills` (public,
   referenced by kasetto.yaml, currently unregistered) is added to `.meta.yaml`.
9. **Proof obligation (phase 4)**: fresh-clone simulation = clone → build envctl → `auto-detect`/`doctor`
   read-only + assert every wired link resolves inside the clone; a full `HOME=/tmp/fakehome`
   materialization is recorded honestly as partial if not run end-to-end.

## Research (mandatory cross-checked)

- **Web — yazelix** (github.com/luccahuguet/yazelix, fetched 2026-06-12): install = `nix profile add
  github:…#yazelix` / home-manager module / `nix run`; `~/.config/yazelix` = user-owned config that
  "persists across updates"; `~/.local/share/yazelix` = "Yazelix-owned output"; `yzx` = the CLI; pinning
  via flake input; fork-friendly child-repo architecture. → D4's config/state split is upstream-sanctioned.
- **Web — Claude Code settings** (code.claude.com/docs/en/settings, fetched 2026-06-12): precedence
  managed > CLI args > `.claude/settings.local.json` > project `.claude/settings.json` > **user
  `~/.claude/settings.json`**; permission rules merge; user CLAUDE.md = `~/.claude/CLAUDE.md` applies to
  all projects; **symlink support is not documented-guaranteed** → D2 includes an empirical smoke test
  (`claude -p` in a scratch dir) after the `~/.claude` inversion, with archived originals as rollback.
- **Code — envctl** (read 2026-06-12): README verb table; `manifest/nix-yazelix.toml` (full stack +
  `shell_rc`/`alternatives`/`nix_conf_lines` wiring kinds, components.d drop-in dir, `ENVCTL_MANIFEST_DIR`);
  `manifest/agent-env.toml` (kasetto wired in, `sync --locked` as verify); `assets/scripts/` contains
  yazelix-setup/config/boot-repair/autoinstall (the external copies are deploys of these); CLAUDE.md
  invariants (pure-Rust trust boundary, engine-only logic, destructive ops fail-closed dry-run).
  envctl repo is **PUBLIC** → per-file review gate before any home config lands (no secrets, no tokens;
  paths/prefs only — gitconfig identity is already public via commits).
- **Code — kasetto** (read 2026-06-12): upstream pivoshenko/kasetto "declarative AI agent environment
  manager"; live global config explicitly assigns the OS/toolchain layer to envctl and warns global
  `mcps:` sync must be verified additive (broker/repowire/weave hand-configured servers).
- **Live state** — `/tmp/recon-portability.out` + `/tmp/portability-sweep.out` (scripts preserved in
  `/tmp/*.sh`): the 3 correct links, the copy inventory with dates proving drift (kst Jun 3 vs Jun 5),
  nix profile entries, systemd unit contents (repowire flap), PATH (6 dup entries + meta paths already
  on PATH via harness_hub), `~/Downloads/tmp/env-config` prior art (pivoshenko dotfiles + claude-code-config).
- **Alternatives rejected**: GNU stow (no detect/verify/lock, foreign tool class envctl already covers);
  bare `ln -s` script at meta root (no idempotence/drift model — envctl `doctor` IS that model);
  `meta env link` subcommand (meta_cli would grow a second materializer; meta already *consumes* envctl
  via `meta dashboard` → envctl dashboard --json).

## Consequences / risks

- Copy-drift class dies (vox×3, kst×2, weave-stale) — `doctor` catches regressions.
- `cargo install` collisions with links: mitigated per D3 (build-from-meta + verify link-ness).
- Public-repo exposure of home configs: mitigated by per-file review + secrets/state exclusions (D5);
  if the user prefers privacy, flipping envctl visibility is a **human** decision (never agent-initiated).
- Claude symlink behavior is empirically gated (D2/D5); failure mode = restore archived original.
- sudo phases (/usr/local/bin links, nix steps) make full unattended bootstrap partial on locked-down
  boxes — recorded in bootstrap output rather than hidden.
- The live session depends on rtk/icm/claude binaries being inverted **last, one at a time, verified after
  each** (HOME-DIR SAFETY rule).

## Cross-references

PORTABILITY-SESSION-PROMPT.md · PORTABILITY-AUDIT.md (phase-1 matrix + gaps) · memoir
`adr-2026-06-11-open-questions` r4 item 16 + `org-audit-verification-2026-06-12` · forgotten-directives
memory (envctl secret relay; `~/Downloads/tmp/handoff` cross-ref; never downgrade) · kasetto.yaml safety
note (live file) · NEEDS-HUMAN.md items 2/3/5 · KB incidents `release-please-token-unavailable`,
`repowire-unit-crash-loop` (new) · envctl docs/{ARCHITECTURE,ROADMAP}.md · ADR-0004 (fleet `.handoff`
— the `~/Downloads/tmp/handoff` bundle is its design source; coordinate before relocating).

## Implementation order (phase 3)

1. This ADR → handoff PR (green gate).
2. yazelix fork + `.meta.yaml` registration (+ agent-skills registration) — meta parent PR.
3. envctl PR: `home/` tree (reviewed file-by-file) + `components.d/` drop-ins (config links, binary
   links, unit links) + yazelix URL flip + repowire disable component note.
4. `meta/scripts/bootstrap.sh` + PORTABILITY-AUDIT.md tracked — same meta parent PR as (2).
5. Apply on-box in order: config links → binary links (rtk/icm last) → `~/.claude` trio (+ claude smoke
   test) → repowire disable. Archive-first every step; one rollback line per inversion.
6. Phase-4 proof + phase-5 records (memoir, KB, NEEDS-HUMAN, SESSION-HANDOFF).
