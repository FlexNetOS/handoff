# Test Matrix

This document describes the comprehensive test coverage for the Continuity Ledger Kernel.

## Unit Tests (148 Total)

### Work-Order Crate
| Test | Purpose |
|------|---------|
| `seam_bundle_to_workorders` | SwarmBundle→WorkOrder conversion |
| `synthesized_orders_are_verifiable_no_junk_defaults` | No path_scope ["."], non-empty test_commands |
| `intake_is_deterministic_same_ids_same_locks` | Same bundle → identical IDs + locks |
| `intent_lock_detects_drift` | Objective mutation detected |
| `roundtrips_through_json` | Serialize/deserialize preserved semantics |
| `legacy_partial_lock_serializes_to_three_fields` | Backward compat (3-field without constraint/northstar) |
| `full_lock_carries_all_five_surfaces` | 5-field lock round-trip integrity |
| `constraint_drift_is_detected_only_on_a_full_lock` | Policy surface drift detection |
| `northstar_revision_drift_is_detected` | Doctrine revision detection |

### Ledger Crate (8 tests)
- Fresh-open schema
- Migration idempotency  
- OLD-schema DB migrates in place + still verifies
- sync_cursor round-trip

### HF CLI Tests (60 tests)
- Command parsing and routing
- Task claim/lease transactions
- Packet generation
- Policy gate enforcement
- Drift detection

## Integration Tests

| Test | Scenario |
|------|----------|
| `hf init` creates structure | Fresh .handoff initialization |
| `hf claim --next` claims safe task | Highest priority backlog claim |
| Overlapping claim is rejected | Lease conflict detection |
| Disjoint claim is allowed | No false positives |
| `hf start` creates worktree | Isolated session branch+worktree |
| `hf checkpoint` records diff | Files changed + command evidence |
| `hf drift` blocks out-of-scope edit | Scope boundary enforcement |

## Drift Tests

### Goal Drift
```rust
// Objective mutated mid-session → hard fail
let mut order = make_task();
assert!(order.intent_unchanged());
order.objective = "Completely different goal".to_string();
assert!(!order.intent_unchanged()); // DETECTED
```

### Scope Drift  
```rust
// Edit outside path scope → hard fail before write
hf drift --path "src/outside_scope.rs"
// Rejected: out_of_scope_files detected
```

### Completion Drift
```rust
// Task marked done without tests → hard fail before handoff
hf handoff
// Blocked: missing_evidence = ["test_commands_executed"]
```

### Context Drift
```rust
// Packet stale vs Git state → reconcile required
git commit --amend  //改变了Git HEAD
hf handoff
// Soft fail: stale_packet_detected = true
// Required action: hf reconcile
```

### Policy Drift
```rust
// Ignoring no-Docker/offline constraint → hard fail before command
hf policy check --command "docker run ..."
// Rejected: policy_violation = ["no_docker"]
```

## Crash Tests

| Scenario | Expected Behavior |
|----------|-------------------|
| Crash during claim | Previous state preserved, recovery task created |
| Crash during checkpoint | Invalid writes rolled back, previous valid state intact |
| Crash during handoff packet write | Old packet preserved, new write aborted |
| Corrupted ledger event | Chain breaks, reconciliation required |

## Concurrency Tests

### Concurrent Writers (Serialized)
```rust
// Agent A: hf claim TASK-0001
// Agent B: hf claim TASK-0001 (same task)
// Result: Only first succeeds; second rejected with conflict error
```

### Disjoint Path Writes (Allowed)
```rust
// Agent A: writes crates/handoff-core/**
// Agent B: writes crates/handoff-ledger/**
// Result: Both succeed, no overlap
```

## Golden Packet Tests

| Scenario | Expected Fields in packet |
|----------|-------------------------|
| Fresh resume | project_name, active_objective, next_command |
| After claim | branch, worktree, claimed_paths |
| After checkpoint | changed_files, commands_run, test_status |
| After handoff | drift_report, next_task_id |

## Performance Benchmarks

| Operation | Expected Latency |
|-----------|------------------|
| `hf resume` (JSON) | < 10ms |
| Task claim (first available) | < 50ms |
| Checkpoint (typical session) | < 200ms |
| Handoff packet generation | < 100ms |

## Test Execution

```bash
# All tests
cargo test --workspace --all-targets

# Specific module
cargo test -p work-order
cargo test -p ledger

# With coverage (if tarpaulin installed)
cargo tarpaulin --workspace
```

## Coverage Goals

| Area | Minimum Coverage |
|------|------------------|
| Work-Order crate | 100% |
| Ledger crate | 100% |
| HF CLI core logic | >95% |
| Policy gates | 100% (all branches tested) |
