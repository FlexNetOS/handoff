# Knowledge Management

Maintain the FlexNetOS knowledge base as you work. Documents are your persistent memory across sessions.

## handoff's local `.kb` (HFTASK-0072, ADR-0018 D7)

handoff has its OWN durable `.kb/` (full git-kb adoption — was code-intelligence-only before). It holds the seven context documents (`context/immutable/{project-brief,patterns,architecture}`, `context/extensible/{product,tech}`, `context/overridable/{active,progress}`) plus tasks/incidents you create. Load it first thing in a session:

```bash
git kb list --path context/      # detect KB state
git kb checkout --path context/  # materialize for reading
git kb board                     # task kanban
```

**Residency (text-vs-binary, same precedent as the `.handoff` ledger / HFTASK-0067):** the DURABLE TEXT is committed (`.kb/store/**` — the markdown documents + JSON commits/refs/manifest); the BINARY DB CACHE (`.kb/.cache/gitkb.db*`) and the EPHEMERAL editing surface (`.kb/workspaces/`) and per-user `.kb/config.toml` are gitignored and rebuilt locally. Never commit a binary `.kb` DB. See `.gitignore`.

**The two-way seam (ADR-0003, ONE-WAY authority):** the planning plane (`.kb`) and execution plane (the `hf` ledger) are bound BOTH directions, but the kb NEVER overrides execution truth:
- **IN:** `hf task mint --from-kb <slug>` — turn a kb task into a witnessed handoff card (`correlation_id = slug`).
- **OUT:** `hf claim`/`checkpoint`/`done`/`release` mirror the card's transition back to the kb task (active / +progress / completed / backlog) — informing the plan, never reading it back as truth.

`hf status` / `hf resume` (the witnessed ledger) is the AUTHORITATIVE loop state; `context/overridable/{active,progress}` are the human-authored kb mirror, not the source of record.

## Before Starting Work

- Check `git kb board` or `kb_board` to see what's active and what's blocked
- If you're about to do non-trivial work and no task exists for it, create one first
- Search before creating: `kb_search` with keywords to avoid duplicates

## While Working

- Add progress entries to the active task document as you make progress
- Include `[[tasks/...]]` wikilinks in git commit messages for related tasks:
  ```
  fix: resolve timeout issue

  Implements [[tasks/HFTASK-42]]
  ```
- When you discover bugs or issues, create incident documents — don't just fix and forget

## After Significant Work

- Update `context/overridable/active` to reflect what changed and what's next
- Check off completed acceptance criteria in task documents
- Add completion evidence (commit hashes, test results) before marking tasks done

## Document Lifecycle

- **Create first, implement second** — the document IS your plan
- **Update as you go** — don't wait until the end to document
- **Complete the body before changing status** — never mark "done" without evidence
- **Link everything** — tasks reference specs, incidents reference fixes, commits reference tasks
