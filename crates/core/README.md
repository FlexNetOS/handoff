# rusty-idd-core (formerly `idd`)

`rusty_idd_core` is the zero-dependency Rust-native control plane for unifying repositories with AI automation. It generates the markdown + JSON contracts, CI gates, and agent templates that human and AI contributors execute to integrate code safely.

This crate is now unified into the **`rusty-idd`** CLI.

## Why this exists

The current safe pattern is not “tell one model to merge everything.” The safer pattern is:

```text
repo map → feature matrix → env/secrets contract → agent queue → small issues → AI PRs → CI/secret gates → serialized integration
```

The hard problems are not text generation. They are:

- preserving working behavior
- preventing secret leakage
- avoiding parallel-agent branch conflicts
- handling the practical one-branch / one-PR shape of cloud coding agents
- making environment/configuration contracts explicit
- keeping a rollback path
- turning “merge these repos” into small, testable vertical slices

## Install

This is a library used by the `rusty-idd` binary. To install the unified CLI:

```bash
cargo install --path crates/cli
```

Or run directly from the workspace root:

```bash
cargo run --bin rusty-idd -- <command>
```

## Commands

All core verbs are available via `rusty-idd`:

```bash
rusty-idd init [path]
rusty-idd scan --repo <path> [--out <file>] [--format md|json]
rusty-idd plan --repo-a <path> --repo-b <path> --out <workspace> [--name <name>]
rusty-idd task --title <title> [--kind <kind>] [--out AI_MERGE/07_tasks]
rusty-idd validate [--workspace <path>] [--report <file>]
rusty-idd manifest [--workspace <path>] [--out .idd/MANIFEST.tsv]
rusty-idd github [--workspace <path>]
```

## Primary workflow

```bash
# 1. Create an integration workspace from two repositories
rusty-idd plan \
  --repo-a ../env-manager \
  --repo-b ../secrets-manager \
  --out ./integration \
  --name env-secrets-unification

# 2. Review generated control files
ls ./integration/AI_MERGE

# 3. Create a narrow task for an AI agent
rusty-idd task \
  --out ./integration/AI_MERGE/07_tasks \
  --kind env-secrets \
  --title "Create canonical SecretProvider and EnvResolver interfaces"

# 4. Validate the workspace before PR work
rusty-idd validate --workspace ./integration

# 5. Refresh the audit manifest after intentional control-plane changes
rusty-idd manifest --workspace ./integration
```

## Generated files

```text
AGENTS.md
SECURITY.md
.env.schema.example.json
.idd/
  LOCK.md
  MANIFEST.tsv
.github/
  copilot-instructions.md
  pull_request_template.md
  ISSUE_TEMPLATE/idd-task.yml
  workflows/idd-ci.yml
AI_MERGE/
  00_repo_a_inventory.md
  00_repo_a_inventory.json
  01_repo_b_inventory.md
  01_repo_b_inventory.json
  02_feature_matrix.md
  03_env_and_secret_contracts.md
  03_env_and_secret_contracts.json
  04_merge_plan.md
  05_conflict_risk_register.md
  06_gap_audit_and_applied_updates.md
  07_tasks/
  08_agent_queue.md
  09_github_execution.md
  10_parity_test_plan.md
  11_provider_matrix.md
```

## Recommended GitHub usage

Use GitHub Issues and PRs as the execution control plane:

1. Paste or link one generated task into one GitHub issue.
2. Assign the issue to the chosen AI agent.
3. Require the PR to update relevant `AI_MERGE` docs.
4. Require CI, tests, lint, `idd validate`, and secret scan before merge.
5. Keep only one integration branch with merge authority.
6. If the agent needs two repositories, import or mirror the second repo into the integration repo first. Do not depend on a single cloud-agent run mutating two independent repos.

## Env/secrets stance

`idd` separates **configuration** from **secrets**:

- configuration keys may be documented in `.env.example` or schema files
- secret values must not be committed
- CI secret references are mapped, not materialized
- validation scans for obvious leaked secret patterns
- OIDC is preferred over static cloud credentials where supported

For real deployments, pair this with a real secret backend such as GitHub Actions secrets, Infisical, SOPS, OpenBao/Vault, Doppler, or a local secure store. `idd` does not replace those systems; it defines the contract agents must obey.

## Design principles

- Rust-native core
- No runtime network calls
- No required external crates
- Deterministic file output
- Agent-readable markdown plus machine-readable JSON sidecars
- GitHub-native issue/PR workflow
- Serialized integration authority
- Additive migration before destructive cleanup
- Backup-on-overwrite preservation

## Status

V2 is a practical, offline-friendly package for repo-unification workflows. It is intentionally small so it can be embedded into larger agentic systems, called from GitHub Actions, or used locally before assigning work to cloud coding agents.
