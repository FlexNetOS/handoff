# TASK-0003 Decision — promote-verify audit check permission

## Context

After TASK-0002 fixed the Windows envctl setup in `promote-verify.yml`, the post-promotion
`Audit` job reached `cargo audit` and found no vulnerabilities. The job then failed while the
`rustsec/audit-check@v2` action attempted to create/update its GitHub check run:
`Resource not accessible by integration`.

## Decision

Grant `promote-verify.yml` explicit workflow permissions:

- `contents: read` for checkout/audit inputs
- `checks: write` for `rustsec/audit-check@v2` to publish the audit check result

## No-downgrade check

This is a minimum-permission CI fix. It does not relax branch protection, does not suppress audit
findings, and does not ignore the existing unmaintained-crate warnings; it only allows the action to
publish its check result instead of failing on GitHub API authorization.
