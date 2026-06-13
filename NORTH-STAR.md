# NORTH-STAR.md

## 1. Purpose

This file is the durable direction layer for the repo.

Every human, agent, sub-agent, swarm, hook, script, and automation reads this before planning or changing the system. It defines what the system is becoming, what must never be violated, how decisions are made, and how work is promoted without corrupting the known-good baseline.

The system is not a collection of apps. It is a local-first, model-native, auditable, reversible agentic operating system.

The operating center is CECCA / NOA: the executive kernel, command layer, root coordinator, and stem-cell agent responsible for preserving system integrity while increasing verified capability.

---

## 2. Immutable North Star

> Build a local-first, auditable, reversible, model-native operating system where every agent action increases verified capability without corrupting the baseline.

The internal objective is simple:

1. **Integrity** — preserve truth, provenance, security, structure, and the known-good world.
2. **Reversibility** — every meaningful change must be snapshot-backed, rollback-safe, and inspectable.
3. **Capability Gain** — every promoted change must measurably improve what the system can safely do.

If a decision improves one pillar while damaging another, it does not promote.

> No promotion without integrity. No promotion without rollback. No promotion without measurable capability gain.

---

## 3. Core Doctrine

The system evolves through proof, not hope.

Pain without learning becomes a loop. Pain with reflection becomes memory. Memory with compression becomes wisdom. Wisdom with action becomes evolution.

Failures are not garbage. Failures are evidence. The system must preserve the gold world, learn from every failed world, promote only proven worlds, and never let a failed world become the new baseline.

---

## 4. Non-Negotiable Invariants

* **Local-first by default.**
* **Offline-capable.**
* **Kernel-first.**
* **Message-passing only.**
* **No global mutable state.**
* **Zero bloat.**
* **Digest everything.**
* **No action without snapshot.**
* **No patch without proof.**
* **No proof without logs.**
* **No promotion without rollback.**
* **No trust in docs over observed runtime.**
* **No demo-only completion.**
* **No hidden dependencies.**
* **No one-way migrations.**

---

## 5. The Gold World Model

The system always protects a known-good world.

### Gold World

The current verified baseline. It is stable, tested, restorable, and must not be casually modified.

### Sandbox World

A temporary experimental space where agents can patch, test, fail, learn, and retry without contaminating the baseline.

### Candidate World

A sandbox world that has passed enough gates to be considered for promotion.

### Failed World

A world that did not pass. It is not deleted blindly. It is compressed into evidence: logs, diffs, failure causes, test cases, blocked paths, and lessons learned.

### Promotion Rule

Only a candidate world that proves integrity, rollback safety, and capability gain may become the next gold world.

---

## 6. Agent Operating Contract

Every agent session starts by constructing a working map of the repo.

Before changing files, an agent must identify:

* current objective;
* repo status;
* active branch or workspace;
* relevant files;
* safe files;
* unsafe files;
* recent changes;
* known failures;
* required tests;
* applicable policies;
* rollback path;
* next safe action.

Agents must operate as bounded executors, not wandering chatbots.

```yaml
handoff:
  objective: ""
  completed: []
  in_progress: []
  next_safe_task: ""
  touched_files: []
  safe_to_touch: []
  do_not_touch: []
  tests_run: []
  tests_required: []
  policy_gates: []
  rollback_plan: ""
  open_risks: []
  next_command: ""
```

---

## 7. Decision Gates

### Intent Gate

The change must map to a real objective.

### Integrity Gate

The change must preserve system truth, structure, security, provenance, and policy.

### Reversibility Gate

The change must be rollback-safe.

### Capability Gate

The change must increase verified capability.

### Promotion Gate

The change may promote only when evidence exists:

* diff summary;
* test results;
* logs;
* rollback plan;
* updated registry or docs when needed;
* capability delta;
* known limitations.

---

## 8. Standard Promotion Workflow

1. **Read** — load North Star, repo state, policies, current tasks, and previous handoff.
2. **Snapshot** — capture current state before modification.
3. **Plan** — define the smallest safe patch.
4. **Patch** — change only what the task requires.
5. **Verify** — run tests, schema checks, lint, build, or targeted validation.
6. **Observe** — collect logs, outputs, diffs, and failures.
7. **Decide** — promote, retry, quarantine, or roll back.
8. **Compress** — convert lessons into tests, policy, docs, or memory.
9. **Handoff** — leave a machine-readable next step.

---

## 9. Anti-Goals

The system must not become:

* a cloud-dependent stack disguised as local-first;
* a pile of apps with no executive kernel;
* a demo that cannot survive real execution;
* a documentation maze that machines cannot use;
* a swarm of agents creating untracked conflicts;
* a brittle automation chain with hidden dependencies;
* a system that deletes failure evidence instead of learning from it;
* a system that optimizes speed while destroying rollback safety;
* a system where the UI becomes the architecture;
* a system where novelty outranks verified capability;
* a system where simulation is accepted as proof.

---

## 10. Final Rule

When uncertain, preserve the gold world.

When something fails, compress the lesson.

When a change cannot prove integrity, reversibility, and capability gain, do not promote it.

