# ADR-0008 — flexnetos_github_app + flexnetos_runner: the GitHub↔local two-plane control system

**Status:** proposed (2026-06-12) · **Owner:** ops plane (handoff-adjacent) · **Derived from:**
plan `mighty-weaving-quasar`, `ARCHITECTURE-TRUTH.md` (62-unit census), ADR-0001 §5/§5a/§9.4
(merge model + gh-aw + the absent merge-gate Environment), envctl `CHARTER.md` + `seam.rs`
(`ProviderMint`), and three hand-verified Explore boundary audits (runner vs atc/loop_lib/handoff;
app vs rusty-idd/handoff; the meta GitHub surface). All load-bearing claims spot-checked against
code/API on 2026-06-12.

## Context

The FlexNetOS org (~60 repos) automates writes through **one long-lived org PAT**
(`PARENT_REPO_PAT`): `clone-child-repos.sh`, `on-child-update.yml`, the `notify-*` dispatch graph.
ADR-0001 §9.4 records the structural gap bluntly — **zero GitHub Environments, all secrets
org-level** (ADR-0001:920-923) — so there is no privilege-separated merge/publish gate, the
trusted writer is over-scoped and never rotates, cross-repo wiring is one-way
(`repository_dispatch` only), and trusted work runs on GitHub-hosted compute. The `2026-06`
public→private CI break is the cautionary tale for how much blast radius a single org-credential
mistake carries.

The org's `.github` umbrella is being **dissolved into peers** (`tasks/github-meta-refactor`;
ARCHITECTURE-TRUTH). Three husk repos were minted to receive the extracted ops concerns —
`flexnetos_github_app`, `flexnetos_runner`, `flexnetos_secrets` — **all empty** (zero commits).

Owner vision: *"a local runner and app to connect all of meta seamlessly."* Law:
**additions only, never downgrades; remove any repo that overlaps existing capability without
adding value.** Verified boundary facts (2026-06-12): **no** GitHub App / webhook /
installation-token code exists anywhere in the fleet (greenfield — fleet grep empty);
`.github_org/runner/` is **shell-only** (`ephemeral-spawn.sh` + systemd templates);
`.github_org/github-app/` is a **manifest scaffold**; **envctl owns the secrets layer** (CHARTER,
`secrets-engine`/`secretd`, ROADMAP P1-5 done) and already defines the GitHub token-minting seam
(`seam.rs` `ProviderMint::mint_scoped`). `flexnetos_secrets` is therefore retired into envctl
(**ADR-0007**), not built.

## Decision — two planes

### §1 Control plane — `flexnetos_github_app` (Rust · axum · local+tunnel)

The org's **privilege-separated GitHub identity, event ingress, and write-authority**. It is the
concrete home for ADR-0001 §5a's *"separate scoped-write job"* and §371's *"non-agent,
Environment-gated merge job."* Crates: `app-core` (lib: webhook payload types + `verify_signature`
HMAC-SHA256 constant-time; App-JWT (RS256) → installation-token exchange; event router; merge-gate
executor = check-run create/update; protected-files denylist — pure, non-printing, table-tested),
`app-server` (bin `fxapp-server`: axum `POST /webhook`, tunnel client, health, structured logs),
`app-cli` (bin `fxapp`: `register|install-smoke|mint-token|replay <payload.json>|doctor`).
**Responsibilities:** webhook ingress; App identity (App ID + private key **sealed in the envctl
vault**); installation-token broker (short-lived, per-repo, per-permission); trusted writer; the
merge-gate executor that posts the gatekeeper verdict **as a required status check**; event router
→ signed dispatch to the runner. **Non-responsibilities (hard boundaries):** holds **no** real
credential (envctl vault does); makes **no** merge *policy* (rusty-idd/handoff own it) — it
*executes* the verdict; performs **no** gatekeeper *judgement* (the reviewer/steward owns it).

### §2 Execution plane — `flexnetos_runner` (Rust · self-hosted · both shapes)

Crates: `runner-core` (lib: job-spec types; the meta-aware **router**; safety policy; JIT/ephemeral
lifecycle), `runner-actions` (bin `fxrun-actions`: self-hosted **GitHub Actions** runner
supervisor — productizes `.github_org/runner/{ephemeral-spawn.sh,register.sh,remove.sh,install.sh,
systemd/*}` into Rust JIT/ephemeral registration with safety rails), `runner-dispatch`
(bin `fxrun-dispatch`: UDS server receiving **signed** job specs from the app), `runner-cli`
(bin `fxrun`: `register|run|status|doctor`). The router is **delegate-only** — it routes work to
the right existing kernel and never reimplements one: `build/test → loop_lib::run` (rayon
fan-out), `agent-task → atc` (executor/registry/queue), `loop-cycle → handoff hf`
(claim/checkpoint/ship + ledger), `lease/a2a → weave`, `worktrees → meta_git_lib`. **Safety
rails:** non-root, no Docker socket, `_work` on tmpfs, **fork-PR isolation** (untrusted fork code
never runs on the self-hosted runner — the verified `atc` `runs-on` gate pattern), label
discipline `[self-hosted, flexnetos]`.

### §3 The seams (frozen contracts)

- **S1 App ↔ envctl (identity/secrets).** The app calls envctl `secretd` over UDS; the installation
  token comes from `ProviderMint::mint_scoped(MintRequest{provider:Github, repos, perms, ttl≤24h})`
  (`seam.rs:41-66`). The App private key is a vault record; the JWT→installation-token exchange
  runs **inside** envctl's mint impl — the app never holds the key. **Build against envctl Phases
  1-5 + the `ProviderMint` seam ONLY** (NOT Phase 6 `inject`/`run`, NOT Phase 8 server-mode).
  Per-action authorization rides envctl broker `decide()` (default-deny).
- **S2 App ↔ handoff ledger (witness).** Every privileged write (token mint, check-run post, PR op,
  merge arm) is recorded as a ledger event (rusqlite + rvf-crypto WitnessChain) — tamper-evident
  provenance for the autonomous loop. State precedence **Git > ledger > cards** (ADR-0001).
- **S3 App ↔ weave (verdict channel).** The merge verdict rides a weave **permission-ask answer
  body + a `review_verdict` event** (ADR-0002 surface 5), **never** a native GitHub APPROVE —
  bot-APPROVE bypasses branch protection (gh-aw #25439, verified ADR-0001:809). The app *surfaces*
  the verdict to GitHub as a required status **check-run**; it does not approve.
- **S4 App ↔ merge model (rusty-idd/handoff).** The **verdict-as-required-status-check IS the gate**
  (ADR-0001:329). Merge = GitHub-native auto-merge (`gh pr merge --auto --squash`) on all-green;
  the merge job is **non-agent + Environment-gated** (the handoff `merge-gate` Environment,
  ADR-0001:536-540). The app supplies that job's scoped token.
- **S5 Runner ↔ kernels (router).** The delegate map in §2. The runner is a bridge, not a
  business-logic executor.
- **S6 Runner ↔ envctl (secrets to child).** Secrets reach a job via an envctl relay-bearer
  overlaid into the **child process env only**; the real key never enters the child
  env/history/git (envctl Phase-3 broker; the FS-S1 `/proc/<pid>/environ` guarantee). Because
  envctl Phase 6 `run`/inject is out-of-bounds, the runner performs the child-env overlay itself
  using a Phase-3-minted bearer (or a `ProviderMint` scoped token) — both within sanctioned phases.
- **S7 App ↔ Runner (dispatch).** The app emits a **signed** job spec over a local UDS; the runner
  verifies the signature before routing. Signing key = an envctl vault record.

### §4 Deployment — local + tunnel

Both run on the local dual-RTX-5090 workstation. Webhook ingress needs public reachability → a
**tunnel** (cloudflared/smee), **not** a public TLS listener and **not** a VPS relay (a VPS would
require envctl Phase 8 SERVER-MODE, which is out-of-bounds). Local-first matches the vision and
keeps the vault USB-gated on-box.

### §5 Separation of privilege (the gh-aw model — verified ADR-0001:803-817)

The reviewing/authoring agent is **read-only** and emits safe-output intents; `flexnetos_github_app`
is the **separate scoped writer** that executes them after a threat-detection pass
(secrets/protected-file/policy scan). There is **no merge "safe output"** — merge is a non-agent,
Environment-gated job. Created PRs are **draft-by-default**; **protected-files denylist** blocks
`.github/`, `CLAUDE.md`, manifests. The human-in-the-loop primitive is *who approves the
`merge-gate` Environment*; swapping that approver from human → code-omniscient AI gatekeeper
(ADR-0005 steward) flips the loop autonomous **without a code change**.

### §6 Security posture (2028 baseline)

Short-lived, per-repo, per-permission **installation tokens** (GitHub default ≤1h, clamped ≤24h by
envctl) replace the long-lived org PAT; **HMAC-SHA256** webhook verification (constant-time);
**fork-PR isolation**; **SLSA build provenance + Sigstore/cosign keyless signing** on
runner-produced artifacts; **no credential custody** in the app or runner (the envctl vault is the
sole keystore). Grounding in Research §B.

## Phases (detail in plan `mighty-weaving-quasar`)

App: P0 scaffold+HMAC+JWT(mock)+health → P1 envctl token seam → P2 live webhook ingress + router +
ledger witness → P3 merge-gate executor → P4 fleet enrollment + `PARENT_REPO_PAT` retirement.
Runner: P0 scaffold+job-spec+router(dry-run) → P1 Actions supervisor (JIT + rails) → P2 meta-native
UDS dispatch → P3 envctl secrets injection + provenance + ledger witness → P4 fleet rollout.

## Consequences

- **Completes the `.github_org` dissolution:** `runner/`→flexnetos_runner, `github-app/`→
  flexnetos_github_app, `secrets/`→envctl (ADR-0007); then `.github_org` slims to roles 1+6.
- **Retires `PARENT_REPO_PAT`** as the trusted writer — removes a standing single point of
  over-privilege.
- **Realizes "connect all of meta seamlessly":** every repo's events → one app → one local runner →
  local execution (shimmy models · envctl secrets · icm memory · handoff/weave/atc orchestration ·
  loop_lib fan-out) → witnessed delivery back through the app.
- **Unblocks the no-human loop:** the app is the missing write-authority arm for ADR-0001 §5a and
  ADR-0005's steward.
- **Risks + mitigations:** runner↔atc overlap → delegate-only router (the runner *calls* atc, never
  dispatches agents itself); ingress exposure → tunnel + HMAC + fork isolation, no inbound public
  port; envctl coupling → the app **fails closed** (clearly-logged refuse) if `secretd` is down,
  never falling back to a plaintext PAT.

## Research / Cross-References

**§A Codebase (hand-verified 2026-06-12).** envctl `crates/secrets-engine/src/seam.rs:41-66`
(`MintRequest`/`ScopedToken`/`ProviderMint::mint_scoped`/`NoMint`; doc *"native scoped sub-token
minting (GitHub fine-grained PAT / App token)"*); `docs/secrets/CHARTER.md` (six-pillar secrets
mission), `ROADMAP.md` (P1-5 done; P6 inject / P8 server-mode out-of-bounds); `crates/{secretd,
secretctl}`, `broker/{decide,policy,token,gate}.rs` (default-deny). handoff
`.handoff/decisions/ADR-0001-loop-upgrades.md:329,362,371,536-540,803-817,920-923` (verdict =
required check; reviewer read-only; non-agent Environment-gated merge; gh-aw separation-of-privilege;
#25439 bot-APPROVE bypass; "0 Environments anywhere"). `docs/adr-0002-weave-a2a-conventions.md`
(surface-5 out-of-band verdict). `.github_org`: `runner/{ephemeral-spawn.sh,register.sh,remove.sh,
systemd/*}`, `github-app/{manifest.example.json,permissions.md}`, `scripts/github-app-token-smoke.py`
(extraction sources). `.meta.yaml:116-137` (the 3 husk repos = "extracted operational concerns").
Fleet grep (excl envctl/RuVector/ruflo): zero `octocrab`/`jsonwebtoken`/installation-token/webhook
code → greenfield. Boundary audits (3 Explore agents, hand-verified): atc = agent dispatcher
(`crates/atc-core/{executor,registry,queue}.rs`); loop_lib = rayon fan-out (`src/lib.rs::run`);
handoff = work-order + witnessed ledger (`work-order/`, `ledger/`); rusty-idd = merge *policy*
(`gh pr merge --auto`, required-check authority).

**§B Web (2025-2026 best practices; sources cited, load-bearing novel claim spot-checked).**

- **GitHub App tokens.** App JWT = RS256 with `exp ≤ 10 min`, `iat −60s` (clock drift); installation
  tokens via `POST /app/installations/{id}/access_tokens` **expire in 1 hour** and **down-scope** by
  `repository_ids` (≤500) + a `permissions` subset → mint *one-repo, one-permission* tokens
  per-operation. **The Checks API is App-only** (`checks: write` is unavailable to PATs) — the hard
  reason the merge-gate verdict path *must* be an App, not the PAT. Installation-token rate budget
  scales with repos (5,000→12,500/hr, 15,000 GHEC) vs a PAT's flat **shared** 5,000/hr.
  [docs.github.com `apps/.../authenticating-*`, `rest/checks/runs`, `rest/overview/rate-limits`]
- **Webhook ingress.** Verify `X-Hub-Signature-256` = HMAC-SHA256 over the **raw UTF-8 body**
  (`sha256=` prefix) with a **constant-time** compare (`hmac.compare_digest`/`timingSafeEqual`);
  dedupe on the `X-GitHub-Delivery` GUID; branch on `X-GitHub-Event`; no proxy may rewrite the body.
  [docs.github.com `webhooks/.../validating-webhook-deliveries`]
- **Merge gate.** Post the verdict via `POST /repos/{o}/{r}/check-runs` (`conclusion:
  success|failure|…`) wired as a **required status check**, then arm `gh pr merge --auto`. **Design
  note (verified 2026-06-12 against the primary source):** an **undocumented, community-reported**
  change (GitHub community discussion #190610) states that **as of ~2026-03-25** auto-merge **cannot
  be enabled until all PR requirements already pass** (HTTP **422** otherwise), contradicting
  still-current docs → the gate must **arm auto-merge AFTER posting the green verdict** and **handle
  422 defensively** regardless. (cli #8206, 2023, is *earlier related friction* — a GraphQL
  "required check is failing" error — NOT itself evidence of the 2026/422 change.) Defend the
  bot-APPROVE bypass (#25439 / Cider Security 2021): keep the verdict a **check-run**,
  never a native APPROVE; ensure the org setting *"Allow GitHub Actions reviews to count towards
  required approval"* is **OFF** (defaults **ON for pre-2022 orgs** → likely a hardening item for
  this org, route to `NEEDS-HUMAN`), require ≥2 reviews + *"require approval of most recent push."*
  [cli.github.com `gh_pr_merge`; docs `rest/checks/runs`; community #190610; cli#8206]
- **JIT / ephemeral runners.** Provision via `POST /orgs/{org}/actions/runners/generate-jitconfig`
  (body `name`/`runner_group_id`/`labels`; returns `encoded_jit_config`); **single-job-then-auto-
  removed** (no long-lived registration token). GitHub: *"self-hosted runners should almost never be
  used for public repositories"* → **fork-PR isolation is mandatory**; non-ephemeral runners can be
  *persistently compromised* (Sysdig runner-backdoor incidents; `tj-actions/changed-files` 2025-03
  supply-chain compromise). Rails: **runner groups**, non-root, **no Docker socket**, egress
  monitoring (StepSecurity Harden-Runner). [docs.github.com `actions/reference/security/secure-use`,
  `rest/actions/self-hosted-runners`; sysdig.com; wiz.io]
- **Provenance / keyless (2028 baseline).** Emit signed **in-toto SLSA provenance** via
  `actions/attest-build-provenance` — Sigstore **keyless** (Fulcio ~10-min cert bound to the runner's
  **OIDC identity** + Rekor transparency log; perms `id-token: write`, `attestations: write`,
  `contents: read`); verify with `gh attestation verify -R <org/repo>`. Honest target on self-hosted
  hardware = **SLSA L2** (signing is the L1→L2 boundary; L3 needs an isolated, author-untamperable
  platform — ephemeral one-job runners help but don't grant L3). Use **GitHub OIDC federation** for
  any cloud egress (short-lived per-job tokens; trust scoped on `sub`/`repository`/`ref`) — the
  2025-2026 keyless direction. [slsa.dev `spec/v1.0/levels`; github.com/actions/attest-build-provenance;
  docs.sigstore.dev; docs.github.com `actions/.../openid-connect`]

**Design deltas folded into the decision above:** App-only check-runs (→ §1 merge-gate is
necessarily the App); mint per-operation 1-repo/1-perm 1-hour tokens (→ S1); arm auto-merge *after*
the green verdict, handle 422 (→ §3 S4, P3); JIT single-job runners + fork isolation (→ §2 rails);
SLSA-L2 keyless provenance + OIDC egress (→ §6). The "Actions-reviews-count-toward-approval"
org-default and ≥2-review hardening are **org-settings → NEEDS-HUMAN** (no unrequested org changes).
