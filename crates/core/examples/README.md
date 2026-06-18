# Example: env manager + secrets manager unification

```bash
idd plan \
  --repo-a ../env-manager \
  --repo-b ../secrets-manager \
  --out ./integration \
  --name env-secrets-unification
```

Generated control plane:

```text
integration/
  AGENTS.md
  SECURITY.md
  .idd/LOCK.md
  .idd/MANIFEST.tsv
  .env.schema.example.json
  .github/copilot-instructions.md
  .github/pull_request_template.md
  .github/ISSUE_TEMPLATE/idd-task.yml
  .github/workflows/idd-ci.yml
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

Then create one GitHub issue per task and assign one AI agent at a time to the integration branch queue.
