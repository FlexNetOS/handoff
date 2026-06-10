# Skill: session-resume

Brought forward (upgraded) from the 2026-06-02 ark_handoff_ledger_v2_package —
see ADR-0001 Research §R9.

## Purpose

Rehydrate current project state for a new agent session with minimal context load.

## Trigger phrases

- resume
- continue
- pick up
- what next
- recover session

## Steps

1. Run `hf resume --json`.
2. Read `.handoff/active.md`.
3. Read `.handoff/context/capsule.json`.
4. Read the active task card under `.handoff/tasks/`.
5. Read the latest ADR(s) under `.handoff/decisions/`.
6. Check the latest drift report (when `hf drift` lands — HFTASK-0005).
7. Print the exact next command.

## Hard rule

Do not edit files during this skill.
