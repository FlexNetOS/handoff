# Gap Audit

date:
time:
audit_id:v2

## [gap-identified] --> [upgrade-implemented]

| Gap | Why it matters | Applied update in v2 |
|---|---|---|
| No GitHub-native agent files | Current cloud coding agents perform better when repository instructions, issue templates, and PR templates are explicit | Added `.github/copilot-instructions.md`, `.github/ISSUE_TEMPLATE/idd-task.yml`, `.github/pull_request_template.md`, and `SECURITY.md` |
| Only markdown output | Agents and automation need machine-readable sidecars, not just prose | Added JSON inventory and env/secrets contract outputs |
| Secret/config scanner was too narrow | Env/secrets managers differ across repos; missing references creates broken runtime behavior or leaks | Added detection for GitHub secrets/vars/env, Node/Vite, Python, Rust, Deno, SOPS, Infisical, Doppler, direnv, mise, Vault/OpenBao, and Compose |
| No explicit agent queue | Parallel agents can collide even when each PR looks reasonable | Added `AI_MERGE/08_agent_queue.md` and stronger lock semantics |
| No GitHub execution plan | A cloud agent session is not a true multi-repo transaction | Added `AI_MERGE/09_github_execution.md` with branch/task model |
| No parity-test plan | Old behavior can disappear during cleanup | Added `AI_MERGE/10_parity_test_plan.md` |
| Provider selection not captured | Environment managers and secret managers get conflated | Added `AI_MERGE/11_provider_matrix.md` |
| Generated files could overwrite prior decisions | Reruns can erase context and create silent drift | Added backup-on-overwrite writes and `.idd/MANIFEST.tsv` |
| Validation missed workflow-specific risks | GitHub Actions has secret and permission traps | Added committed dotenv, `write-all`, `pull_request_target`, and `secrets.*` in `if:` checks |

## Out of scope

`idd` does not call GitHub APIs, create PRs, store secrets, or choose a cloud provider. That is deliberate. The package creates the contract and gates; the chosen agent or human contributor executes the task.

## Review Notes

### What was Missed | Overlooked

1. GitHub-native agent wiring was incomplete. V1 had `AGENTS.md` but did not generate repository instructions, issue templates, PR templates, or a GitHub execution plan.
2. The scanner under-detected env/secrets references. It handled only a few common cases and missed Python, Vite/import-meta, bracket access, GitHub vars/env, SOPS, Infisical, Doppler, direnv, mise, Vault/OpenBao, and Compose env files.
3. There was no machine-readable control plane. V1 generated markdown only, which is readable but harder for scripts and downstream agents.
4. There was no manifest or preservation behavior. Rerunning generation could overwrite prior decisions without a built-in backup trail.
5. The merge plan did not explicitly model cloud-agent constraints: one issue, one branch, one PR, serialized integration authority.
6. Parity testing was referenced but not operationalized into a concrete control-plane file.
7. Provider selection was not isolated. Secret manager vs environment manager choices needed a provider matrix before implementation.
8. Validation missed committed dotenv files and common workflow risk shapes.

### Upgrades Added | No Downgrades

- Added GitHub agent templates.
- Added agent queue, GitHub execution plan, parity test plan, and provider matrix.
- Added JSON sidecars.
- Added deterministic manifest command.
- Added backup-on-overwrite file generation.
- Expanded env/secrets scanning.
- Expanded validation gates.
- Updated docs and examples.

## Known limitation

This package was statically reviewed in this environment. The active container did not include `cargo` or `rustc`, so compilation could not be executed here. The code is written dependency-free and includes tests, Cargo metadata, and a lockfile for direct verification on a Rust toolchain.