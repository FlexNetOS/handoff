# Spec Engine Design — `crates/spec` (rusty-idd)

The Rust-native port of the OpenSpec intent-driven lifecycle engine. This is a
**design document only** (Slice 0). No code, no `Cargo.toml`, no `crates/spec/`
exists yet — that is Slice 4. The authoritative behavior this design must
reproduce is in `lifecycle-contract.md` and the golden fixtures in
`oracle-fixtures/`.

Tags as in the contract: **`oracle-verified`** (we have a captured fixture the
implementation must match) vs **`schema-only`** (hand-authored from docs).

---

## 1. Crate matrix (decided — do not re-litigate)

| Concern | Crate | Why this, not the alternative |
|---------|-------|-------------------------------|
| Markdown AST + delta-merge | **`comrak`** | True *mutable* AST + `format_commonmark` round-trip — needed to locate `### Requirement:` H3 nodes and splice subtrees. `pulldown-cmark` is an event stream (no mutable tree); its round-trip via `pulldown-cmark-to-cmark` is lossy by design. pulldown is acceptable only for read-only header scans (list/status). |
| YAML (schema.yaml, config.yaml, .openspec.yaml) | **`serde_norway`** | Maintained `serde_yaml` fork (mdBook ecosystem). **NOT `serde_yaml`** (archived 2024). **NOT `serde_yml`** (unsound — **RUSTSEC-2025-0068**). |
| CLI surface | **`clap` v4 (derive)** | The lifecycle has many subcommands (validate/archive/show/status/instructions/templates/...); derive is the de-facto standard. `lexopt`/`pico-args` only if matching a strict no-clap house style. |
| Artifact scaffolding (templates) | **`minijinja`** (preferred) or **Tera** | Emit proposal/design/tasks/spec/adr templates. minijinja = lighter, fewer deps; Tera if richer Jinja features are needed. |
| Gherkin scenarios | **none** | Scenarios are Markdown-wrapped, not `.feature` files. The `gherkin`/`cucumber` crate parses `.feature` — wrong format. GIVEN/WHEN/THEN is a **content-lint** target inside the comrak AST (and the contract verified validate does NOT enforce it). |
| Errors | `thiserror` (lib) at the edge | Domain model stays error-type-light; map at boundaries. |
| Serde | `serde` (+ derive) at the edge only | Keep serde derives **off** the pure model structs. |

**Core-crate purity discipline (hexagonal).** The *pure model* (requirement /
scenario / delta structs + the merge algorithm operating on an in-memory tree
abstraction) must be dependency-light: no `comrak`, `serde`, `clap`, or I/O in
it. Parsing (comrak), YAML (serde_norway), templating (minijinja), the CLI
(clap), and filesystem access live at the crate **edges** (the `parse`,
`render`, `io`, `cli` modules). The zero-dep invariant belongs to
`crates/core`, not here — `crates/spec` legitimately carries comrak/serde_norway
— but the *model submodule* keeps the dependency wall so the merge logic is unit
-testable without a Markdown parser in the loop.

---

## 2. Module layout

```
crates/spec/
  Cargo.toml                # comrak, serde_norway, clap, minijinja, thiserror, serde (edge deps)
  src/
    lib.rs                  # re-exports; no logic
    model/                  # PURE, dependency-light (no comrak/serde/io)
      mod.rs
      requirement.rs        # Requirement, Scenario
      delta.rs              # Delta, DeltaOp, RenamePair
      spec.rs               # SpecDoc (ordered requirements + header/purpose)
      merge.rs              # apply_delta(SpecDoc, Delta) -> Result<SpecDoc, MergeError>  (the crux; tree-abstraction in, tree-abstraction out)
    parse/                  # EDGE: comrak AST <-> model
      mod.rs
      spec_parser.rs        # comrak AST -> SpecDoc (scan H3 "Requirement:", H4 "Scenario:")
      delta_parser.rs       # comrak AST -> Delta (scan "## <OP> Requirements")
      emit.rs               # SpecDoc -> Markdown via format_commonmark (byte-stable)
    validate/               # EDGE: structural validation -> ValidationReport (mirrors oracle JSON)
      mod.rs
      rules.rs              # scenario>=1 (ERROR), SHALL/MUST (ERROR), H4 scenario, Purpose>=50 (WARNING)
      report.rs             # ValidationReport/Item/Issue (serde at edge) -> matches fixture JSON
    archive/                # EDGE: orchestrates merge + move (transactional)
      mod.rs                # apply each delta atomically; abort-all-on-any-failure; then move dir
    schema/                 # EDGE: serde_norway load of schema.yaml -> ArtifactGraph
      mod.rs                # Artifact{id,generates,template,requires}, ApplyStep
      graph.rs              # DAG: requires-edges, topo order, "ready next artifact"
    scaffold/               # EDGE: minijinja render of templates -> artifact stubs
      mod.rs
    adr/                    # EDGE: ADR file scan + supersession-graph walk
      mod.rs                # parse Status/Supersedes; compute in-force set; next NNNN
    cli/                    # EDGE: clap derive -> dispatch to the above
      mod.rs
  tests/
    golden/                 # differential fixtures (see §6)
```

`model/` is imported by `parse`, `validate`, `archive` but imports none of them.
That is the dependency wall.

---

## 3. Domain model (pure)

```rust
// model/requirement.rs
pub struct Requirement {
    pub name: String,            // text after "### Requirement: "
    pub body: Vec<Block>,        // prose between heading and first scenario (model-level block, NOT comrak nodes)
    pub scenarios: Vec<Scenario>,
}
pub struct Scenario {
    pub name: String,            // text after "#### Scenario: "
    pub steps: Vec<Block>,       // GIVEN/WHEN/THEN bullets as opaque blocks (content-lint, not parsed semantically)
}

// model/spec.rs
pub struct SpecDoc {
    pub title: Option<String>,   // "# <name> Specification"
    pub purpose: Option<Vec<Block>>,
    pub requirements: Vec<Requirement>,  // ORDERED; order is load-bearing (RENAMED keeps position, ADDED appends)
}

// model/delta.rs
pub enum DeltaOp {
    Added(Requirement),
    Modified(Requirement),       // carries the WHOLE replacement block
    Removed { name: String, reason: Option<String>, migration: Option<String> },
    Renamed { from: String, to: String },
}
pub struct Delta { pub ops: Vec<DeltaOp> }  // grouped under ## ADDED/MODIFIED/REMOVED/RENAMED Requirements
```

`Block` is a small model-owned representation (kind + raw CommonMark text span)
so the model never depends on `comrak::nodes`. The parser fills `Block`s; the
emitter renders them back. This keeps `merge.rs` parser-free.

`name` equality for op matching is **whitespace-insensitive on the heading
text** (`oracle-verified` — the contract's MODIFIED matches the base header
ignoring whitespace). Implement a `normalize_name()` (trim + collapse internal
whitespace) used by all op lookups.

---

## 4. Artifact state machine

Driven by `schema/graph.rs` over the `serde_norway`-loaded `schema.yaml`
(`schema-only` — edges read straight from the schema):

- Nodes = artifacts `{proposal, specs, design, adr, tasks}` + the `apply` phase.
- Edge `a -> b` iff `b.requires` contains `a`.
- An artifact is **ready** iff every artifact in its `requires` set is `done`.
  `done` = the artifact's `generates` glob has been produced (and, for `specs`,
  validates with no ERROR).
- `apply` (phase, gated on `tasks`) iterates `tasks.md` checkboxes; `- [x]`
  count / total drives progress. `apply.tracks = tasks.md`.
- The state machine exposes: `next_ready(state) -> Option<Artifact>` (for a
  `continue`/`ff` command) and `is_archivable(change) -> bool` (all artifacts
  `done`, used by `status`/`archive` warnings).
- Validate cross-check is **per-file only** — the state machine does NOT block
  archive on a MODIFIED-not-found; that is caught in the archive merge (§5),
  matching the contract's separation of validate vs archive.

---

## 5. Delta-merge / archive algorithm (the crux) — in comrak terms

`oracle-verified` against `oracle-fixtures/{01-base, 02-delta}` → `03-archived`.

**Parse phase** (`parse/spec_parser.rs`, base spec):
1. `comrak::parse_document` the base `spec.md` into an AST arena.
2. Walk top-level children. Track the current section. For each **H3** node whose
   inline text, normalized, starts with `Requirement:`, open a new requirement
   span; collect following nodes until the next H3 or end as that requirement's
   subtree. Within it, each **H4** `Scenario:` opens a scenario span.
3. Build `SpecDoc` with the **ordered** `Vec<Requirement>` and a
   `name -> index` map (normalized names).

**Parse phase** (`parse/delta_parser.rs`, delta spec):
1. Walk for **H2** nodes whose text is `ADDED|MODIFIED|REMOVED|RENAMED
   Requirements`; the op kind is the leading word.
2. Under ADDED/MODIFIED: parse contained H3 `Requirement:` blocks into
   `Requirement` (full block). Under REMOVED: parse the H3 name + the
   `**Reason**` / `**Migration**` paragraphs. Under RENAMED: parse the
   `- FROM: \`...\`` / `- TO: \`...\`` list items, stripping the
   `### Requirement: ` prefix.

**Merge phase** (`model/merge.rs`, pure — operates on `SpecDoc` + `Delta`):
Apply ops **transactionally**. Validate *all* preconditions first (or use a
working clone and discard on any error) so a mid-merge failure changes nothing,
matching `oracle-verified` `"Aborted. No files were changed."`:
- **ADDED** → precondition: name absent (else `MergeError::AlreadyExists`,
  matching CLI `"... already exists"`). Effect: **append** to the end of
  `requirements`. (`oracle-verified`: JSON export appended last.)
- **MODIFIED** → precondition: name present (else `MergeError::NotFound`,
  matching CLI `"... not found"`). Effect: **replace the whole `Requirement`
  in place** (keep index). The delta's `Requirement` is the entire new block;
  the old scenarios are discarded. (`oracle-verified`: rate-limit 10→20, scenarios
  fully replaced.) ⭐ Do **not** attempt scenario-level merge — that is the
  agent-driven `sync`, not `archive`.
- **REMOVED** → precondition: name present. Effect: remove the `Requirement`
  from the `Vec`. (`oracle-verified`: Legacy XML gone.)
- **RENAMED** → precondition: `from` present, `to` absent. Effect: change
  `requirement.name` **in place** (keep index, keep body+scenarios).
  (`oracle-verified`: "Export filename" → "Exported file naming", in position.)

Order of application within a single archive: ADDED-exists and
MODIFIED/RENAMED/REMOVED-not-found are evaluated against the **base** state; the
safest faithful implementation validates every op against base, then applies. (A
later Slice-4 probe can confirm whether the oracle evaluates ops sequentially vs
against-base when a delta both renames X and modifies X; flagged below.)

**Emit phase** (`parse/emit.rs`):
1. Rebuild a comrak AST from `SpecDoc` (or splice the original arena — see
   risk §7) and `format_commonmark`.
2. **Tune the formatter for byte-stability** against fixture `03`: the oracle
   **tightens blank lines** (removed the blank line after `## Purpose` body and
   after `## Requirements`), preserves single blank lines between scenarios, and
   ends with a trailing newline. Configure comrak `ComrakOptions`
   (width=0/no-wrap, consistent list bullet `-`, etc.) and, if needed, a thin
   post-pass to match exactly. **The golden test is byte-for-byte.**

**Archive orchestration** (`archive/mod.rs`):
1. (default) run validate; abort if ERRORs (unless `--no-validate`).
2. For each delta spec under the change: load base, parse delta, merge,
   emit; collect `+added/~modified/-removed/→renamed` counts.
3. If any spec merge errors → abort **all**, write nothing.
4. Move `changes/<change>/` → `changes/archive/YYYY-MM-DD-<change>/`.
5. `--skip-specs` → step 1–3 skipped, just the move.

---

## 6. Fixture-based / differential test plan

The engine is proven by **golden fixtures from the Node oracle**, NOT by
old-vs-new parity (there is no old Rust path). QA's pass criterion is the oracle
diff.

Seed fixtures (already captured, in `oracle-fixtures/`):
- `01-base-spec.md` + `02-delta-spec.md` → **must produce byte-identical**
  `03-archived-result.md`. Exercises all four ops + append/in-place ordering +
  formatter blank-line tightening in one shot.
- `04-validate-spec.json` — `validate <spec> --json` of a passing-with-WARNING
  spec; rusty-idd's `--json` must match shape + level + message + summary.
- `05-validate-no-scenario.json` — failing-with-ERROR spec; must match the
  ERROR path/message and `valid:false`.

Test layers:
1. **Merge unit tests** (pure `model/merge.rs`): per-op (ADDED append,
   MODIFIED whole-replace, REMOVED, RENAMED), and the four error modes
   (ADDED-exists, MODIFIED/RENAMED/REMOVED-not-found) → `MergeError`, asserting
   **no mutation** on error (transactionality).
2. **Golden archive test**: `(01 + 02) -> assert_eq!(emit, 03)` byte-for-byte.
3. **Validate golden tests**: feed the negative/positive specs, assert the
   serialized report equals the captured JSON (modulo `durationMs`, which the
   test should null out).
4. **Round-trip stability**: `parse(emit(parse(x))) == parse(x)` and
   `emit` is idempotent (archiving an already-merged spec with an empty delta is
   a no-op byte-wise).
5. **Differential oracle harness** (Slice-4, dev-only): a script that, for a
   corpus of generated `(base, delta)` pairs, runs both `bunx
   @fission-ai/openspec@latest archive` and rusty-idd and diffs the outputs;
   any divergence is a bug. Pin **v1.4.1**. Then Node is dropped from the
   shipped product.

Expand the corpus in Slice 4 to cover: multi-capability changes, REMOVED
Reason/Migration presence, RENAMED+MODIFIED of the same requirement, and the
blank-line edge cases.

---

## 7. Risks / open implementation questions

- **Formatter byte-stability** ✅ **resolved (U6).** Rather than tune
  `format_commonmark`, `parse/emit.rs` rebuilds the document with the oracle's
  exact blank-line rules directly (H1 + blank; `## Purpose` section fully tight;
  per-requirement: heading tight to body, blank before each scenario, blank after
  each requirement → the trailing blank line). Byte-exact golden tests
  (`archive_emit_is_byte_identical_to_oracle`, and the same for fixture `08`)
  lock it against `03`/`08`. The semantic golden remains the primary gate.
- **Splice-in-place vs rebuild.** Two emit strategies: (a) mutate the original
  comrak arena (preserves untouched nodes' exact formatting), or (b) rebuild the
  AST from `SpecDoc`. (a) risks less reformatting drift but is fiddlier; (b) is
  cleaner but must reproduce formatting from scratch. Prototype (a) first against
  fixture `03`; fall back to (b) if arena splicing fights the borrow checker.
- **Op-evaluation order** (RENAMED then MODIFIED of the same req in one delta):
  ✅ **resolved** (`oracle-verified`, U5 probe of `@fission-ai/openspec@1.4.1`).
  The oracle applies **RENAME first**: a delta that renames `X→Y` and modifies
  the same requirement must reference the NEW header `Y` in MODIFIED; referencing
  the old `X` aborts ("when a rename exists, MODIFIED must reference the new
  header ..."). `merge.rs` applies RENAMED ops before ADDED/MODIFIED/REMOVED and
  rejects old-name references via `MergeError::RenamedOldNameReferenced`.
  Fixtures `06`/`07`/`08`; tests in `model/merge.rs` + `tests/archive_golden.rs`.
- **REMOVED Reason/Migration** enforcement is `schema-only`; decide whether to
  hard-error or content-lint. Default to content-lint (matches observed merge).
- **serde_norway model leakage.** Keep YAML deser structs in `schema/`, mapped
  into pure model types — don't let `#[derive(Deserialize)]` reach `model/`.

---

## 8. Single biggest risk for the implementation slice

**Byte-for-byte formatter fidelity of `format_commonmark` against the oracle's
re-serialization.** The merge *logic* is well-specified and verified; the place
Slice 4 will actually bleed time is making comrak emit whitespace identical to
the Node CLI (which tightens blank lines around section headers, fixture `03`).
This is the load-bearing unknown — resolve it first with the captured golden
fixture before building op handlers, because every archive golden test depends
on it.
