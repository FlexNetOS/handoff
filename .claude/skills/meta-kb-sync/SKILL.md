---
name: meta-kb-sync
description: "Keeps the handoff kernel in sync with the meta workspace's shared loop/worktree engine (loop_lib, meta_git_lib), host CLI + conventions (meta_cli, .meta.yaml), and the .kb planning↔execution seam (ADR-0003, one-way ledger→kb). ALWAYS use when a task touches the loop/session/worktree engine, cross-repo registration/conventions, kb minting, or kb write-back. Do NOT use for within-repo card/packet drift (that's drift-reconcile) or per-repo .handoff rollout (that's fleet-handoff)."
---

# meta-kb-sync — handoff must track the meta engines, host CLI, and .kb seam

handoff is a meta member. Three surfaces drift if left alone; this skill is how each
stays coherent. The governing principle is **sync, don't reimplement**: the right
fix for engine drift is a *dependency on the shared crate*, never a parallel copy.

## Surface 1 — loop / worktree engine (`loop_lib` + `meta_git_lib`)

| Engine | What it owns | handoff should… |
|--------|-------------|-----------------|
| `loop_lib` | run commands across directories (the meta loop engine; `meta_cli` depends on it) | share its execution semantics, not fork a second loop |
| `meta_git_lib` | worktrees: `git_ops` add/remove, `store` TTL registry, `hooks` fire_post_create/destroy, `helpers::{resolve_branch, ensure_worktrees_in_gitignore}`, snapshot capture/restore | back `hf session start\|end [--recycle]` (HFTASK-0007) |

**Drift check:** read `hf/Cargo.toml`. Today it depends on `work-order`, `ledger`,
`serde`, `serde_json`, `toml` — **not** loop_lib/meta_git_lib. That means the loop
currently reinvents session/worktree logic. The coherent fix:
- `hf session` depends on `meta_git_lib` (worktree engine); fall back to the
  `meta git worktree` CLI only if the lib isn't wired.
- session works off `origin/<base_branch>` (from `policy.toml`), reserves a weave
  path-scope lease, emits `session_start`/`session_end`, recycles a fresh set on end.
Flag any *semantic* divergence between the handoff loop and loop_lib (cycle model,
parallelism, error handling) as a finding even where a direct dependency isn't taken.

## Surface 2 — host CLI + conventions (`meta_cli` + org set)

- **Registration (idempotent):** handoff is already in `../.meta.yaml`
  (`tags: [orchestration, handoff]`) + `../.gitignore`. There is no
  `meta project add` — edit those files with **grep-guarded** edits, never blind
  append; clean the known duplicate `.gitignore` line. Never re-add what's present.
  **SSH default:** every `.meta.yaml` `repo:` is `git@github.com:FlexNetOS/<name>.git`
  (SSH) — register new members in that form, never `https://github.com/…` (HTTPS fails
  the workspace's auth).
- **Convention set (HFTASK-0016):** commitlint.config.cjs (12 types) +
  semantic-pr-title (merge-blocking); release-please manifest mode + VERSION +
  5-platform release.yml (NOT cargo-dist); renovate.json (NOT Dependabot);
  `.githooks/{commit-msg,pre-commit,pre-push}` + `make install-hooks` (NOT python
  pre-commit); Makefile (NOT Justfile); `.claude/agent-guard.toml`. Detect what's
  missing; do not reintroduce the rusty-idd drift these explicitly reject.
- **Reachability:** `hf` should run under `meta exec` without colliding with `meta`
  command semantics.

## Surface 3 — the .kb seam (ADR-0003): planning ⟷ execution

Two planes, one identifier chain, **one-way authority**.

| Rule | Mechanism | Failure if skipped |
|------|-----------|--------------------|
| **Plane charter** | git-kb = planning (`/kb-board`, what/why/next); `.handoff` = execution (claims/leases/evidence). Execution precedence: Git > ledger > cards | two boards drift (defect D3: 3 kb tasks vs 22 stale cards) |
| **Minting (in)** | `hf task mint --from-kb <slug>` → stamps `kb_ref` + `correlation_id`, computes IntentLock | work nobody can trace to a plan |
| **Write-back (out)** | `hf checkpoint`/`hf handoff` → progress line on the kb task; `hf claim` → `active`; terminal `done` → `completed` **with evidence** | planning board goes stale |
| **Single-registry** | kb owns "what's next"; cards are derived snapshots via `hf checkpoint --sync-cards` | a second source of truth |
| **Binding** | kb slug → card `correlation_id` → weave job → PR → merge commit; commits carry `[[tasks/<kb-slug>]]` + card id | broken traceability |

**The one-way invariant (do not violate):** kb is **never read back into the
witnessed ledger**. The planning plane informs; it never overrides execution truth.
The one-way mirror (HFTASK-0011): write only the generated slugs
`context/overridable/active` + `context/overridable/progress` from ledger-derived
content — git kb has no upsert, so do show-or-create → checkout → full-overwrite →
commit, **preserving frontmatter id** (never rm+recreate).

**Degrade and say so:** a repo with no `.kb` → minting falls back to card-only with
a warning (the ClaimGate convention). Log it; never fabricate a kb_ref.

## Procedure each cycle

1. Does the cycle's task touch the engine, conventions/registration, or the kb seam?
   If none, emit a one-line "no meta-sync surface this cycle" and stop.
2. For each touched surface, run its drift check above (read the real files:
   `hf/Cargo.toml`, `../.meta.yaml`, `../.gitignore`, the kb board, the referenced
   kb task).
3. Map repairs to their kernel tasks (engine→HFTASK-0007, kb mirror→HFTASK-0011,
   conventions→HFTASK-0016) and route them through the implementer + gatekeeper —
   never edit a sibling repo or `.meta.yaml`/`.kb` without a witnessed verdict.
4. Write `_workspace/07_metasync_<scope>.md` (coherence table + actions/tasks).

## Ledger residency invariant (ADR-0004 §3 + envctl gate)

There are exactly **two orchestration-home ledgers**: **FLEET** `meta/.handoff/
ledger.db` (fleet/member events) and **KERNEL** `meta/handoff/.handoff/ledger.db`
(handoff self-dev). Per-repo `.handoff/` dirs are **git-text-only — no `ledger.db`,
no binary state**; their events checkpoint into the FLEET ledger and their packets
are compiled centrally by `hf fleet status` (unbuilt). This is the envctl agenticOS
"ledger-residency ($META_ROOT only)" gate. A stray `<repo>/.handoff/ledger.db` is a
policy-P7 violation — flag it for removal.

## Safety

- Cross-repo edits: snapshot first (`meta git snapshot create`), `--include` the
  target, preview with `meta --dry-run exec`. Idempotent, grep-guarded config edits.
- kb write-back is one-way only — if you ever find kb content flowing into the
  ledger, that's a P0 authority-model corruption: stop and escalate.
- Never create a per-repo `ledger.db` (and never `hf init`/`hf seed` inside a fleet
  repo — those do). The shipped `hf` is the S1 spike missing `fleet`/`policy`/`drift`/
  `sync`; the fleet-aware rendering that makes residency work is carded as HFTASK-0007/
  0011 + `hf fleet status` (ADR-0004 §4) — gaps, not things to route around.
