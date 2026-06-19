# rusty-idd Integration Plan (No Code Refactor)

## Strategy: COPY + REFERENCE

Rather than adding a new peer repo, copy rusty-idd into handoff as `crates/intent-analysis/`.

## Changes Required (Minimal)

### 1. Copy Repository
```bash
cd /home/drdave/Desktop/meta/handoff
cp -r /home/drdave/Desktop/meta/rusty-idd crates/intent-analysis/
# Update gitignore to exclude .idd if present
```

### 2. Cargo.toml Workspace Member
Add to `Cargo.toml`:
```toml
members = [
    # ... existing members ...
    "crates/intent-analysis"
]
```

### 3. hf CLI Integration Point
Add to `hf/src/main.rs`:
- New subcommand: `hf index --intent-aware`
- Logic: spawn rusty-idd subprocess, capture JSON output

## Data Flow (CLI Mode)

```
hf index --intent-aware
    ↓ spawns rusty-idd as subprocess
rusty-idd emits JSON intent map
    ↓ handoff captures JSON
handoff merges into .handoff/context/capsule.json
```

## Future Enhancement (P3 - Optional)

| Task ID | Title | Priority |
|---------|-------|----------|
| HFINTENT-001 | Implement semantic intent hash in IntentLock | P3 |

**Enhancement adds:** `implementation_semanic_hash` field to IntentLock struct

## Why This Approach?

1. **No code refactoring** - rusty-idd stays as-is
2. **No new peer repo** - one less thing in meta workspace
3. **CLI integration** - loose coupling, minimal maintenance
4. **Backward compatible** - existing hf index works unchanged

## Git Considerations

- Keep handoff's .gitignore unchanged
- rusty-idd's .gitignore stays local to copied directory
- No git subtree/merge needed - pure file copy
