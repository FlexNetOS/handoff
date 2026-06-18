# rusty-idd Integration Research

## Overview
**Repository:** `/home/drdave/Desktop/meta/rusty-idd` (peer to handoff in meta workspace)  
**Note:** This is the **correct** rusty-idd - a peer repo in the same meta parent directory

## What It Does
**Intent-driven implementation analysis for AI coding agents:**
- Deep code understanding beyond surface syntax
- Semantic intent detection (what code is trying to do)
- Graph-based implementation analysis
- Designed specifically as a tool for AI agent assistance

## Relationship to Handoff System
**Peer repos in the same meta workspace** - they complement each other:
- **handoff**: Session state management, conflict control, drift resistance
- **rusty-idd**: Intent-driven code understanding, implementation analysis

## Relevance to Handoff System: HIGH

### Direct Match - Intent Detection
Our system uses **IntentLock** to detect when work drifts from the original objective:

```json
{
  "objective_hash": "blake3:...",
  "path_scope_hash": "blake3:...",
  "acceptance_hash": "blake3:..."
}
```

rusty-idd provides **implementation-level intent detection** - what the code is actually trying to do.

### Synergy Points

1. **hf index enhancement**: Could use rusty-idd for implementation-aware repo mapping
2. **IntentLock augmentation**: Could incorporate implementation intent analysis  
3. **DRIFT CONTROL**: Detect when implementation intent diverges from source code intent

## Advantages Over Current Implementation

| Aspect | Current Approach | rusty-idd Integration |
|--------|------------------|----------------------|
| Code understanding | Surface-level (file structure) | Semantic (what code does) |
| Intent detection | Hash comparison only | Automated semantic analysis |
| Analysis depth | File-based | Graph-based |

## Integration Points

### 1. hf index enhancement
```
hf index --intent-aware
└─> Could integrate rusty-idd
    └─> Generates implementation context
        └─> Enhances context capsule
```

### 2. IntentLock augmentation
**Before:** Only detects API surface drift  
**After:** Could detect implementation intent drift

```rust
// Standard intent lock (current)
intent_lock.objective_hash == current_objective_hash

// Enhanced with rusty-idd (future)
implemented_intent != recorded_implementation_intent  // NEW
```

### 3. DRIFT CONTROL
Detect when the **implementation intent** has changed but objective hasn't.

## Recommendation: INTEGRATE INTO META WORKSPACE

**Status:** ✅ Research complete  
**Priority:** P2 (enhancement after core stability)

### Why Integrate?
The intent-driven aspect **exactly matches** our IntentLock drift control system philosophy. rusty-idd is a peer repo designed for the same use case.

### Current Status
- Both repos are in `/home/drdave/Desktop/meta/`
- handoff handles session state and conflict control
- rusty-idd handles implementation analysis
- Complementary functionality, not overlapping

## Future Tasks

| Task ID | Title | Priority |
|---------|-------|----------|
| INTEGRATE-RUSTY-IDD-001 | Analyze collaboration points with handoff | P2 |
| ENHANCEMENT-RUSTY-IDD-001 | hf index --intent-aware flag | P3 |
| ENHANCEMENT-RUSTY-IDD-002 | Integrate implementation intent into IntentLock | P2 |

