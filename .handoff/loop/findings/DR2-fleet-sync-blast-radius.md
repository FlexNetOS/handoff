# DR2 · fleet-sync-blast-radius — findings

Dimension: blast radius of adding `pub fn cmd_fleet_sync(json, dry_run)` to `handoff-fleet` +
a `fleet sync` / `fleet status --fix` dispatch arm in `hf/src/main.rs`, driving
`scripts/handoff-loop-init.sh` per non-conformant member (= HFTASK-0087, depends on HFTASK-0085).

Map basis: `.handoff/loop/reports/codemap-delta-2026-06-26.md` §3. All claims confirmed by direct
source read (the codemap's index-degradation note: git-kb cross-crate edges are stale post-split,
so the call graph was NOT trusted here).

---

## Q1 — Private-field access (in-crate read is the clean path)

**CLAIM 1.1 — `Row` and ALL its flag fields are private; only an in-crate function can read them.**
`struct Row` has no `pub` (`handoff-fleet/src/lib.rs:95`); every flag field is bare-private:
`jsonl_export_missing` (`:106`), `tracked_ledger` (`:110`), `ledger_guard_missing` (`:113`),
`walshm_guard_missing` (`:116`), `per_repo_chain` (`:123`). `PerRepoChain` is private too (`:127`).
Confidence: **high**.

**CLAIM 1.2 — `collect_rows` returns `Vec<Row>` and is callable in-crate (private `fn`).**
`fn collect_rows(root: &Path, members: &[String]) -> Vec<Row>` at `:228`; it is module-private
(no `pub`) but `cmd_fleet_status` already calls it at `:305`. A new `pub fn cmd_fleet_sync` placed
in the SAME `lib.rs` can therefore call `collect_rows` and read `r.jsonl_export_missing` etc.
**directly with zero visibility changes** — Rust private items are visible throughout the
defining module. Confidence: **high**.

**DESIGN CONSEQUENCE:** put `cmd_fleet_sync` in `handoff-fleet/src/lib.rs` (the card's stated
target). Do NOT pub-ify `Row` / its fields just to reach them — that would widen the crate's API
surface for no caller. The only thing `hf` needs is the `pub fn cmd_fleet_sync` entrypoint, exactly
mirroring how `cmd_fleet_status` (`:298`, `pub`) is the sole public hook for the read path. A
flagged-member selector should be a private helper:
`fn flagged(r: &Row) -> bool { r.jsonl_export_missing || r.tracked_ledger || r.ledger_guard_missing || r.walshm_guard_missing }`
— mirroring the existing text-board `match` at `:459-470` and the warning filters at `:317-340`.

---

## Q2 — Script path resolution

**CLAIM 2.1 — `find_meta_root()` returns the meta root (dir holding `.meta.yaml`), NOT the handoff
repo.** `find_meta_root` walks up from cwd for `.meta.yaml` and returns that dir
(`handoff-fleet/src/lib.rs:37-47`). The handoff repo is a member *under* that root (`<root>/handoff`).
So the script is at **`<meta_root>/handoff/scripts/handoff-loop-init.sh`**, and a member dir is
`<meta_root>/<member>`. Confidence: **high**.

**CLAIM 2.2 — the script accepts positional member-path TARGET args and runs per-member.**
Arg parsing (`scripts/handoff-loop-init.sh:62-76`): any non-`--` token is pushed to `TARGETS`
(`:74`); `--fleet` (`:66`) is the *opt-in* for all members. The main loop iterates
`for dir in "${TARGETS[@]}"` (`:293`) doing per-target init/guard/hooks/diff-drive/relay/rules.
If no target and no `--fleet`, it defaults to the current git toplevel (`:144-147`). So
**a single member path can be passed** (`handoff-loop-init.sh <meta_root>/<member>`) to remediate
exactly one member — `--fleet` is not required and would over-reach. Confidence: **high**.

**CLAIM 2.3 — the deploy bits the card names all run per single-target.** Inside the loop:
`ensure_ledger_guard` (`:314`), `deploy_hooks` (`:340`), `deploy_diff_drive` (`:344`),
`deploy_session_relay` (`:347`), `deploy_rules` (`:350`) — every one takes `"$dir"` and is
idempotent (byte-compare-then-overwrite for relay/rules; plain copy for diff-drive). The HFTASK-0085
staleness rebuild is **Phase 0** (`:81-131`), keyed off `KERNEL_HOME`/`HANDOFF_KERNEL_HOME` and run
ONCE before the target loop (not per-member). Confidence: **high**.

**CLAIM 2.4 — script locating is a NEW coupling but already has a self-locate convention to reuse.**
The script self-resolves `KERNEL_HOME` from `SCRIPT_DIR/..` and exports `HANDOFF_KERNEL_HOME`
(`:44-58`). The Rust side has NO equivalent today — `cmd_fleet_status` never shells to a script.
The robust Rust resolution: `let script = root.join("handoff").join("scripts").join("handoff-loop-init.sh");`
(root from `find_meta_root`), with an env override `HANDOFF_KERNEL_HOME` honored first (so an
ejected/relocated kernel still resolves), and a **fail-closed existence check** before spawning
(missing script ⇒ error + non-zero, never a silent no-op). Confidence: **high** on the path shape;
**medium** on the exact override precedence (depends on the ejected-harness story — flag for design).

---

## Q3 — Mutator concern (spawning a deploy script is the real new invariant)

**CLAIM 3.1 — spawning a process is NOT new; `handoff-fleet` already shells out 6×.** All read-only
`git` invocations via `std::process::Command` (imported `:32`): `git_tracks_handoff_db` `:157`,
`git_tracks_jsonl_export` `:173`, `ledger_guard_present` `:199`, `walshm_guard_present` `:220`, and
the two test helpers `:740,818`. So `Command` is established. Confidence: **high**.

**CLAIM 3.2 — but every current spawn is READ-ONLY; `cmd_fleet_sync` makes the crate a MUTATOR of
member repos.** Today `cmd_fleet_status` writes nothing (it only reads filesystem + `git ls-files`/
`check-ignore`); `collect_rows` is documented "pure read" (codemap §3). Running
`handoff-loop-init.sh` *deploys files into member repos* (gitignore guard, hooks, workflows, skills,
rules) and can `cargo install` (Phase 0). This is a **genuinely new invariant for the crate** — it
crosses from observer to actor. Confidence: **high**.

**CLAIM 3.3 — there IS a kernel precedent for a Rust verb shelling to scripts / external bins.**
`hf/src/main.rs` runs external processes from verbs in several places — e.g. the claim hint prints a
`grit-shared.sh` cycle (`:937-940`), `cmd_test` runs the card's `test_commands`, and `hf` shells
git throughout. The closest *script-driving* analog is the loop-init's own caller surface (the script
is the canonical deploy mechanism — `scripts/handoff-loop-init.sh` is invoked today from skills/CI,
not from `hf`). So `cmd_fleet_sync` shelling to the script is **consistent with the kernel's
"the bash script is the deploy mechanism, Rust orchestrates" pattern**, but it is the FIRST time a
`handoff-*` *library crate* (vs the `hf` bin) drives a mutating script. Confidence: **medium-high**
(precedent for shelling exists; precedent for a fleet *library* mutating member repos does not).

**DESIGN CONSEQUENCE:** the `--dry-run` flag in the signature maps directly onto the script's own
`--dry-run` (`:72`, `:79` `run()` gate, dry branches throughout). A Rust `dry_run=true` MUST pass
`--dry-run` to the script so the unit/integration path mutates nothing. Capture child stdout/stderr
and surface per-member; do NOT inherit-and-detach (the report needs the result).

---

## Q4 — Witness / ledger

**CLAIM 4.1 — no current fleet code emits a ledger event; `cmd_fleet_status` is witness-free.**
`cmd_fleet_status` (`:298-487`) only `Ledger::open(...).all_events()/verify_witness_chain()/
verify_rollup_provenance()` — pure reads (`fleet_ledger_stats:268`, `fleet_provenance:287`,
`per_repo_chain_stats:135`). No `append`. The loop-init deploys are git-text only and emit **no**
ledger event (codemap DR2 note (c)). Confidence: **high**.

**CLAIM 4.2 — the kernel doctrine is "every mutating transition is witnessed, fail-closed", and the
primitive is in-reach.** `must_witness<T>(r, what)` (`handoff-core/src/lib.rs:119-126`) wraps a
ledger result and `exit(1)`s on failure (the FAIL-OPEN ban, LESSONS L7-L10); mutating verbs use it
~13× in `hf/src/main.rs` (e.g. claim transition `:927`, `pr_opened` `:1929`). `handoff-fleet`
already depends on `ledger` (`handoff-fleet/Cargo.toml:10`) and on `handoff-core` (`:9`), so
`handoff_core::must_witness` + `ledger::Ledger::append(event_type, work_order_id, payload, ts_ns)`
(`ledger/src/v1.rs:504`) are both callable **with no new dependency**. Confidence: **high**.

**CLAIM 4.3 — the FLEET ledger is the right witness target, not a per-repo ledger.** A fleet-level
remediation is a meta-root action; the FLEET ledger lives at `<meta_root>/.handoff/ledger.db`
(`fleet_ledger_stats:269`, `fleet_provenance:288`, packet render `:540`). A per-member sync result
could ALSO be witnessed into that member's gitignored `<member>/.handoff/ledger.db` (the rollup
source), but the authoritative record of "the fleet self-healed member X" belongs in the FLEET
ledger. Confidence: **medium-high** (the two-ledger residency model — ADR-0004 §3 — supports either;
the cleaner choice is one `handoff.fleet_sync.v1`/`fleet_sync` event per remediated member into the
FLEET ledger, plus the report).

**DESIGN CONSEQUENCE (fail-open trap):** the doctrine says do NOT silently proceed. But the FLEET
ledger may be **ABSENT** when running from a non-meta context (`fleet_ledger_stats` returns
`(0,0,false)` then). Two honest options: (a) witness fail-closed via `must_witness` only when the
FLEET ledger is present, and emit an explicit "ledger absent — sync NOT witnessed" line otherwise
(degrade-with-evidence, not silent); (b) require the FLEET ledger and fail-closed if absent.
`--dry-run` MUST witness nothing. This is the single highest-risk design decision — see Risk 1.

---

## Q5 — Test surface

**CLAIM 5.1 — there are 5 `#[test]`s; 3 build temp git/ledger member repos with reusable inline
helpers.** `parses_member_keys_under_projects_only` (`:625`, pure parse),
`member_packet_is_capsule_driven_not_hardcoded` (`:648`, pure compose),
`fleet_status_verifies_per_repo_chain_and_provenance` (`:670`, temp meta-root + central+member
`Ledger`), `p7_flip_tracked_ledger_and_guard_detection` (`:727`, temp git repo + `.gitignore`),
`p7_inversion_requires_tracked_jsonl_export` (`:805`, temp git repo + JSONL). Confidence: **high**.

**CLAIM 5.2 — the reusable helper is the inline `git` closure + the `temp_dir/format!(pid-nanos)`
isolated-repo idiom (NOT a shared fn).** Each filesystem test builds its own isolated repo via
`std::env::temp_dir().join(format!("hf-…-{pid}-{nanos}"))` (`:730-737`, `:808-815`) and a local
`let git = |args| Command::new("git").args(["-C", repo])...` closure (`:739-745`, `:817-823`),
then asserts flag-detection (`git_tracks_handoff_db`, `ledger_guard_present`, etc.) and
`remove_dir_all` teardown. There is **no extracted `mk_temp_repo()` helper** — the pattern is
copy-pasted per test. Confidence: **high**.

**CLAIM 5.3 — a `--dry-run` unit test CAN reuse the idiom for selection + no-op idempotence, but
CANNOT exercise the real shell-out as a unit test.** A unit test can: build a temp meta-root with N
member repos (some conformant, some flagged) using the `:727`-style git/`.gitignore` setup, then
assert `collect_rows` + the new `flagged()` selector picks exactly the non-conformant set, and that
`cmd_fleet_sync(json, dry_run=true)` mutates nothing (re-`collect_rows` unchanged). It canNOT
unit-test the actual deploy: the real script needs `HANDOFF_KERNEL_HOME`/the `hf` binary and writes
files — that is an **integration test** (codemap DR2 note (d)). Confidence: **high**.

**DESIGN CONSEQUENCE:** factor the flagged-member selection (pure, over `&[Row]`) so it is unit
testable without spawning anything. The card's `test_commands` is just `cargo test`
(`HFTASK-0087.task.json:15`), so the unit `--dry-run` test satisfies acceptance; the live shell-out
should be a guarded/ignored integration test or driven by the differential-drive harness.

---

## Q6 — `handoff.fleet_sync.v1` JSON shape (mirror `fleet_status.v1`)

`cmd_fleet_status` emits `schema: "handoff.fleet_status.v1"` with `meta_root`, `fleet_ledger{…}`,
`members[]{name, …flags…}`, `warnings[]`, printed via `handoff_core::pretty_json` (`:354-397`).
`cmd_fleet_sync` should mirror it so consumers/tests parse one family:

```json
{
  "schema": "handoff.fleet_sync.v1",
  "meta_root": "<root>",
  "dry_run": true,
  "members_total": N,
  "members_flagged": K,
  "remediated": [
    { "name": "memberx",
      "before": { "jsonl_export_missing": true, "tracked_ledger": false,
                  "ledger_guard_missing": true, "walshm_guard_missing": false },
      "actions": ["ensure_ledger_guard","deploy_hooks","deploy_rules", "..."],
      "script_exit": 0,
      "after":  { "jsonl_export_missing": false, "tracked_ledger": false,
                  "ledger_guard_missing": false, "walshm_guard_missing": false },
      "resolved": true,
      "witnessed_event_seq": 412 }
  ],
  "failures": [ { "name": "membery", "error": "...", "script_exit": 2 } ],
  "warnings": [ "..." ]
}
```

Exit-code contract (mirror the status/render arms `:3797-3822` and the per-member fail-closed
mandate): **exit 0** when every flagged member resolved (or none flagged); **non-zero** when any
member's remediation failed or any `after` row is still flagged — but the sweep MUST run all members
first (one member's failure never aborts the others; collect into `failures[]`). Confidence:
**high** on the shape; **medium** on exact exit semantics (design choice: does an unresolved-but-no-
error member count as failure? — recommend yes, since detection→remediation is the card's contract).

---

## Blast radius summary

**Impacted files (all within `path_scope: ["spike/**","handoff/**"]`, `HFTASK-0087.task.json:8-11`):**
- `handoff-fleet/src/lib.rs` — NEW `pub fn cmd_fleet_sync(json, dry_run)` + private `flagged(&Row)`
  selector + a `handoff.fleet_sync.v1` JSON/text renderer + `must_witness`/`Ledger::append` witness.
  Reuses `collect_rows` (`:228`), `find_meta_root` (`:37`), `parse_members` (`:53`),
  `fleet_ledger_stats` (`:268`). **No dependency change** — `ledger` + `handoff-core` already present
  (`handoff-fleet/Cargo.toml:9-11`).
- `hf/src/main.rs` — NEW `Some("fleet") if …=="sync"` (and `status` + `--fix`) dispatch arm beside
  `:3797-3822`; usage string update; the HFTASK-0087 `test_commands` already seeded
  (`cmd_seed:3100-3118`). Alias `use handoff_fleet as fleet;` (`:44`) already routes `fleet::…`.
- `scripts/handoff-loop-init.sh` — DRIVER, already capable of single-member + `--dry-run`
  (`:62-76,144-147,293-369`); likely **no change** needed (verify `ensure_ledger_guard`/HFTASK-0085
  rebuild reachability from the `hf`-spawned context).
- (no change) `handoff-fleet/Cargo.toml` — contrary to the codemap's "possibly" note, witnessing and
  spawning need no new crate.

**Caller/callee deltas:**
- New caller edge: `hf::main` dispatch → `fleet::cmd_fleet_sync` (one site, mirrors the lone
  `cmd_fleet_status` caller at `:3798`).
- New callee edges inside the crate: `cmd_fleet_sync` → `collect_rows` `:228`, `find_meta_root` `:37`,
  `parse_members` `:53`, `fleet_ledger_stats` `:268`, `handoff_core::must_witness`,
  `ledger::Ledger::{open,append}`, plus `std::process::Command` → bash (NEW: first MUTATING spawn).
- `collect_rows` gains a 2nd in-crate caller (was single — `cmd_fleet_status:305`); its signature is
  unchanged so no impact to existing callers.

---

## Top 3 risks (each forces a design decision)

1. **Fail-OPEN on an absent FLEET ledger / unwitnessed remediation.** The kernel bans silent
   proceed-when-precondition-unconfirmed (LESSONS L7-L10; `must_witness:119`). A fleet sync that
   mutates member repos but can't (or doesn't) witness because the FLEET ledger is absent would be
   exactly that class of bug. **Decision forced:** witness each remediation into the FLEET ledger
   fail-closed via `must_witness` when present, and when absent emit an explicit "NOT witnessed"
   degrade line (or hard-fail) — never a silent success. `--dry-run` witnesses nothing.

2. **A library crate becoming a member-repo mutator (least-privilege + reversibility).**
   `handoff-fleet` is today a pure observer; `cmd_fleet_sync` lets it deploy files into and
   `cargo install` for arbitrary members (Phase 0). A bug or a bad script makes the fleet self-modify
   destructively, with the "no human in the loop" doctrine meaning no review gate. **Decision
   forced:** default to `--dry-run`-safe semantics, pass `dry_run` through to the script's `--dry-run`,
   scope to single-member spawns (not blanket `--fleet` from inside `hf`), capture+report child output,
   and fail-closed per member (collect into `failures[]`, never abort the sweep) per the card.

3. **Script-location coupling / silent no-op when the script is unreachable.** The crate's 4 deps
   carry no script-path knowledge; the only locator is the bash self-resolve (`:44-58`). If the Rust
   path-join is wrong (ejected kernel, non-meta cwd, renamed dir) the spawn could fail or silently
   no-op — a fail-open in disguise. **Decision forced:** resolve `<meta_root>/handoff/scripts/
   handoff-loop-init.sh` (honor `HANDOFF_KERNEL_HOME` first), **existence-check before spawn**, and
   error loudly with non-zero exit if missing — never treat "script not found" as "nothing to do".
