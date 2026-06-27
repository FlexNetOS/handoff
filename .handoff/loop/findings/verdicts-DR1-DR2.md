# Verdicts — DR1 (crate-decomposition-state) + DR2 (fleet-sync-blast-radius)

Verifier pass: 2026-06-26. Default-skeptical refutation of the material claims driving the
in-flight HFTASK-0087 (`hf fleet sync`) change. Each verdict rests on a direct read of the cited
`file:line` (not the analyst's quote) plus, for DR1's open caveat, actually running the gates.

## Verdict table

| # | Claim | Verdict | Evidence (read directly) |
|---|-------|---------|--------------------------|
| DR2-Q1 | A new `pub fn cmd_fleet_sync` in `handoff-fleet/src/lib.rs` can read the PRIVATE `Row` flag fields and call `collect_rows()` directly (same module). | **CONFIRMED** | `struct Row {` at `lib.rs:95` — no `pub`. Flags bare-private: `jsonl_export_missing:106`, `tracked_ledger:110`, `ledger_guard_missing:113`, `walshm_guard_missing:116`, `per_repo_chain:123`. `fn collect_rows(root: &Path, members: &[String]) -> Vec<Row>` at `:228` — module-private, **returns `Vec<Row>`**, already called in-crate by `cmd_fleet_status` at `:305`. Rust private items are visible throughout the defining module, so a sibling fn reads `r.jsonl_export_missing` etc. with zero visibility changes. `cmd_fleet_status` is the existing `pub` hook (`:298`). |
| DR2-Q2 | `scripts/handoff-loop-init.sh` accepts a single member-directory positional arg and deploys to just that member — `--fleet` NOT required. | **CONFIRMED** | Arg loop `for a in "$@"` (`:64-76`): any non-`--` token → `TARGETS+=("$a")` (`:74`); `--fleet` is opt-in (`:66`); unknown `--*` flag → `exit 2` (`:73`). Target resolution: `--fleet` expands all members (`:141-143`); **if `TARGETS` empty, defaults to `git rev-parse --show-toplevel`** (`:144-147`) — so a bare path is honored and is the ONLY target. Main loop `for dir in "${TARGETS[@]}"` (`:293`) runs per-target: `ensure_ledger_guard` (`:314`), `deploy_hooks` (`:340`), `deploy_diff_drive` (`:344`), `deploy_session_relay` (`:347`), `deploy_rules` (`:350`), then `hf resume`/`hf drift` (`:354-355`). A bare `handoff-loop-init.sh <meta_root>/<member>` deploys to exactly that one member. The per-member-path design is **not** refuted. |
| DR1 constraint | `handoff-fleet` CANNOT depend on `hf` (cycle: `hf` aliases `handoff_fleet as fleet`), so `cmd_fleet_sync` must use only `handoff-core`/`ledger`/`work-order`. | **CONFIRMED** | `hf/Cargo.toml:33` → `handoff-fleet = { path = "../handoff-fleet" }`; `hf/src/main.rs:44` → `use handoff_fleet as fleet;`. `handoff-fleet/Cargo.toml` deps = `handoff-core` (`:9`), `ledger` (`:10`), `work-order` (`:11`), `serde_json` (`:12`) — **no `hf`**. A back-edge `handoff-fleet → hf` would be a build cycle. Helpers must come from `handoff-core`, which is exactly where `must_witness`/`pretty_json`/`run_out`/`ledger_path` were lifted (module doc `handoff-core/src/lib.rs:117-118`: feature crates witness "without depending back on the `hf` binary crate"). |
| DR2-Q4 | FLEET ledger lives at `<meta_root>/.handoff/ledger.db`; `must_witness`/`Ledger::append` reachable from handoff-fleet; deps include `ledger`+`handoff-core`; `must_witness` is `pub`. | **CONFIRMED** | `fleet_ledger_stats` resolves `root.join(".handoff").join("ledger.db")` (`lib.rs:269`); `find_meta_root` returns the `.meta.yaml` dir = meta root (`:37-47`). `handoff-fleet/Cargo.toml` deps `handoff-core` (`:9`) + `ledger` (`:10`). `pub fn must_witness<T>(r: ledger::Result<T>, what) -> T` at `handoff-core/src/lib.rs:119` (exits 1 on failure — fail-closed). `Ledger::append` reachable via the `ledger` dep. No new dependency needed. |

## DR1 open caveat — CLOSED by running the gates (baseline before the change)

Ran the kernel's exact CI gate on the crate as it stands today:

| Gate | Command | Exit | Result |
|------|---------|------|--------|
| Build | `cargo build -p handoff-fleet` | 0 | clean (62 crates compiled; "Finished") |
| Clippy | `cargo clippy -p handoff-fleet --all-targets -- -D warnings` | 0 | "No issues found" (no `-D warnings` trip) |
| Test | `cargo test -p handoff-fleet` | 0 | **5 passed**, 0 failed (tests-ran = 5 > 0; matches DR2 CLAIM 5.1's "5 `#[test]`s") |

**`handoff-fleet` is GREEN at baseline** — build + clippy(`--all-targets -D warnings`) + test all exit 0,
tests-ran positively counted (not a vacuous "0 tests" pass). DR1 Claim 4.1/4.2 upgrade from
"compiles clean" to **CI-gate clean** for this crate. (Scope: `-p handoff-fleet` only, as tasked; the
full `--workspace` gate was not re-run here — DR1 §4 already ran `cargo check --workspace` = exit 0.)

## Refutations / design-affecting findings

No material claim is REFUTED. One QUALIFICATION that affects the DR2 implementation design:

- **QUALIFIED — `script_exit` is NOT a reliable per-member failure signal (DR2 Q6 JSON shape).**
  `scripts/handoff-loop-init.sh` ends with an unconditional `exit 0` (`:374`), and a per-member
  deploy failure inside the loop does `FAIL=$((FAIL+1)); continue` (e.g. `:306`) **without changing
  the process exit code**. Non-zero exits (`exit 2`) only occur for *pre-loop* fail-closed conditions
  (unknown flag `:73`, NEEDS-HUMAN no-kernel `:113`). The script also silently `continue`s past a
  non-git-repo target (`:294-295`). **Consequence:** the proposed `"failures":[{"script_exit":2}]`
  shape is misleading — a Rust caller spawning the script per-member will almost always see
  `script_exit==0` regardless of internal failure. The DR2 design's **`after` re-`collect_rows`
  snapshot + `resolved` boolean is therefore LOAD-BEARING**, not merely a nicety: it is the only
  reliable signal that a flagged member was actually remediated. The Rust verb's own exit code MUST
  be computed from the `after` flags (per DR2's stated exit contract), never inherited from the
  script. This strengthens (does not contradict) DR2's Risk 3 (fail-open / silent no-op) — the script
  cannot be trusted to report failure via exit code. Recommend: parse `script_exit` for the *pre-loop*
  fail-closed cases (script unreachable / NEEDS-HUMAN), but gate remediation success on the `after`
  re-check, and treat a still-flagged `after` row as failure even when `script_exit==0`.

- **Note (not a verdict change):** DR2's choice to pass `--dry-run` through to the script's own `--dry-run`
  (`:72`, `run()` gate `:79`) is sound — confirmed the script's `DRY=1` path echoes instead of executing
  the deploy/`cargo install` steps (`:79,116,304,313,327`). `--dry-run` mutating nothing holds.

## Net for the synthesizer / implementer

All four structural preconditions for HFTASK-0087 are CONFIRMED on real source, and the target crate
is verified GREEN at baseline. The design as specified in DR1/DR2 is implementable as written, with the
single load-bearing refinement that **member-remediation success must be judged by the `after`
re-`collect_rows` snapshot, not the script's exit code** (which is always 0 for in-loop failures).
