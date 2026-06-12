# ADR-0007 — retire `flexnetos_secrets`; secrets are envctl's single source of truth

**Status:** proposed (2026-06-12) · **Owner:** ops plane / envctl · **Derived from:** the owner's
mission directive (*"if a repo overlaps and conflicts to the point it does not add value then
remove it — e.g. flexnetos_secrets may conflict with envctl that holds the secrets"*), envctl
`CHARTER.md` + `seam.rs` + `ROADMAP.md`, ARCHITECTURE-TRUTH.md (husk D4), and the
`tasks/github-meta-refactor` umbrella dissolution. Load-bearing claims hand-verified 2026-06-12.

## Context

`flexnetos_secrets` is an **empty husk** (zero commits) minted during the `.github` umbrella
refactor to "extract operational concerns" (`.meta.yaml:123-125`). Its intended concern —
secret storage, rotation, and credential brokering for the org's automation — is **already owned,
in production, by envctl**:

- envctl `CLAUDE.md` declares a first-class **"secrets stack"**; `docs/secrets/CHARTER.md` states
  the mission verbatim: *"Be the secrets/security layer… a local, single-operator secrets vault +
  credential injector,"* across six pillars (security, keys, certs, auto-inject, database, api).
- The implementation is built and tested: `crates/secrets-engine` (pure-Rust XChaCha20-Poly1305
  vault, Argon2id/HKDF dual-KEK keyslots, default-deny broker, USB-gated relay, hash-chained
  audit), `secretd` (tonic gRPC daemon over UDS), `secretctl` (client), `secrets-store-libsql`.
  `ROADMAP.md` shows **Phases 1-5 done** (vault, keymgmt, broker/relay, certs, daemon).
- envctl **already defines the exact GitHub-token concern** `flexnetos_secrets` would have built:
  `crates/secrets-engine/src/seam.rs:41-66` `ProviderMint::mint_scoped(MintRequest{provider, repos,
  perms, ttl}) -> ScopedToken`, doc-commented *"native scoped sub-token minting (GitHub fine-grained
  PAT / App token)"* (currently the `NoMint` default — a defined seam awaiting its GitHub body).

Building `flexnetos_secrets` would therefore duplicate a more capable, tested system and **split
secrets across two sources of truth** — precisely the split-brain a single-operator security layer
must not have, and a **downgrade** under the standing *additions-only* law.

## Decision — remove `flexnetos_secrets`; envctl is the one secrets plane

1. **De-register** `flexnetos_secrets` from `.meta.yaml` (`projects.flexnetos_secrets`) and from the
   root `.gitignore` child-repo entry. This is **reversible and non-org-mutating** (no GitHub
   change). Confirmed absent from `.github/scripts/clone-child-repos.sh` (no edit needed there).
2. **The GitHub-token concern → envctl** `ProviderMint::mint_scoped` GitHub variant (App-JWT →
   installation token, ≤24h, per-repo/per-permission, USB-gated via the broker). This is the
   sanctioned **`PARENT_REPO_PAT` replacement** and is consumed by `flexnetos_github_app`
   (ADR-0008 seam S1). It lands inside envctl's Phase-3 broker scope (sanctioned; not Phase 6/8).
3. **Migrate the legacy ops** `.github_org/secrets/**` (the GPG `pass`-style store, `.gpg-id`
   roots, and `scripts/secrets-{inject,rotate,mirror-to-bws,sync-github-from-bitwarden}.sh`) **into
   envctl's vault** via `secretctl import`, retiring the GPG/Bitwarden path. The `secrets-rotate.yml`
   / `reusable-secrets.yml` ops glue is absorbed by envctl's rotation surface
   (`docs/secrets/ops/04-backup-rotation.md`), not re-homed to a peer repo.
4. **Archiving the empty `FlexNetOS/flexnetos_secrets` GitHub repo is an org action → user-authorized
   only.** Record the exact ask in `NEEDS-HUMAN.md`; **do not auto-archive** (org-mutation rule).

## Consequences

- Resolves the secrets leg of the `.github_org` dissolution **into envctl**, not a new peer —
  one authoritative keystore.
- Advances GAP-REGISTER **#15** (envctl secret injection/relay) and clears ARCHITECTURE-TRUTH
  husk **D4** for secrets.
- Adds a small, well-scoped envctl backlog: the `ProviderMint` GitHub body (separate task; the
  unblocker for ADR-0008 P1) and the legacy `.github_org/secrets` import.
- Leaves `flexnetos_brain`, the two wikis, `assets`, and the empty hubs as **separate** husk
  decisions (not in scope here).

## Research / Cross-References

**Codebase (hand-verified 2026-06-12):** `git -C flexnetos_secrets ls-files` → 0 files, no commits.
envctl `crates/secrets-engine/src/seam.rs:41-66` (`ProviderMint::mint_scoped`, GitHub App-token
doc); `docs/secrets/CHARTER.md` (secrets mission + six pillars + "not multi-tenant, one operator");
`docs/secrets/ROADMAP.md` (Phases 1-5 done; broker/relay = Phase 3; inject Phase 6 / server-mode
Phase 8 explicitly deferred); `crates/{secretd,secretctl,secrets-engine/broker}`. `.github_org`
`ls-files`: `secrets/store/**/*.gpg`, `secrets/.gpg-id*`, `scripts/secrets-*.sh`,
`.github/workflows/{secrets-rotate,reusable-secrets}.yml` (the legacy ops being folded in).
`.meta.yaml:123-125` (registration to remove). ARCHITECTURE-TRUTH.md (husk census, D4). ICM
memoir `context-envctl-secrets` (01KTQRX55 — the secrets-engine deep-dive). **Principle:** a
secrets system's value is inversely proportional to the number of authoritative copies of a secret;
two stores = double the leak surface and guaranteed drift. Consolidating on the more-capable,
audited, USB-gated vault is the only non-downgrading move. **Web (2025-2026 direction):** industry
practice converges on a *single authoritative store minting short-lived, scoped, attested
credentials* — GitHub App installation tokens (≤1h), OIDC workload-identity federation, Sigstore
keyless — rather than distributing long-lived secrets across repos; envctl's ≤24h USB-gated
relay/mint is the on-prem embodiment, which a second `flexnetos_secrets` store would only dilute.
[docs.github.com `.../openid-connect`; slsa.dev; docs.sigstore.dev]

**Cross-references:** ADR-0008 (consumes envctl `ProviderMint` for the App's installation tokens) ·
ADR-0001 §9.4/§9.5 (envctl relay as the `PARENT_REPO_PAT` replacement) · GAP-REGISTER #15 ·
`tasks/github-meta-refactor`.
