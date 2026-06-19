# User Story: AI Agent Session Continuity

## Persona
**Role:** AI Coding Agent  
**Need:** To resume work after context limits without losing project state  
**Goal:** To have a durable, replayable session handoff system that any agent can use

## Situation
After working on a complex code task, the agent's context window fills up. When continuing later:
- Previous conversation is lost
- No clear record of what was being worked on
- Unclear which tasks are safe to pick up
- Risk of re-implementing already-done work or missing critical constraints

## Solution
The Continuity Ledger Kernel provides a repository-local handoff system that:

1. **State Preservation** - All project state is in Git + ledger.db + task cards
2. **Immediate Resumption** - Any agent can run `hf resume` and know exactly where to start
3. **Conflict Prevention** - Transactional claims and leases prevent overlapping work
4. **Drift Detection** - Intent locks detect when work has veered from scope

## Acceptance Criteria
1. Run `hf resume` and immediately know state without human help
2. Claims are transactional and conflicts are blocked
3. Drift is detected before handoff with hard gates
4. All completion evidence is recorded in the tamper-evident ledger
5. Fresh agents (no prior chat context) can complete full workflow

## Workflow Example
```bash
# Resume from fresh shell (no chat history)
hf resume
# → Shows active objective, pending tasks, next command

# Claim a task safely
hf claim TASK-0042 --next
# → Transactionally reserves path scope and branch

# Make changes in isolated worktree
hf start
git checkout agent/TASK-0042/...
# → Changes isolated from other agents

# Checkpoint progress
hf checkpoint --note "fixed null check"
# → Records files changed, commands run, test status

# Verify before handoff
hf drift
hf handoff

# Next agent resumes seamlessly
hf resume --json
```

## Pain Points This Solves
- ❌ **Lost context** → ✅ Ledger replays all events
- ❌ **Race conditions** → ✅ Lease-based coordination  
- ❌ **Scope creep** → ✅ Intent locks detect drift
- ❌ ** undocumented work** → ✅ ADRs required for changes

## Success Metrics
- Time to resume from fresh state: < 30 seconds
- Conflict rate: 0 (all overlapping writes blocked)
- Drift detection: 100% of scope violations caught before handoff
