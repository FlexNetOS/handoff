# rusty-idd — security advisory ledger

Tracks `cargo audit` findings for the workspace, the doctrine applied (STRICT UPGRADE-ONLY;
no unmaintained dep left in the tree if a maintained successor exists), and the disposition of
each. The machine-enforced policy lives in `.cargo/audit.toml`.

Last reviewed: **2026-06-26**.

## Standing policy

- **Vulnerabilities** (`RUSTSEC` with a non-empty `patched`): never ignored — upgrade or replace.
- **Unmaintained** (informational): FIX upgrade-only if a maintained successor exists; otherwise
  record here as accepted-risk **with a watch**, and ignore in `.cargo/audit.toml` so the gate
  stays honest-green on the genuinely-blocked item only.

## Findings

### RUSTSEC-2024-0320 — `yaml-rust 0.4.5` unmaintained — ✅ FIXED (2026-06-26)

- **Entered via:** `syntect 5.3.0` (`yaml-load`) ← `comrak` / `tui-markdown` ← rusty-idd crates.
- **Fix:** `[patch.crates-io] syntect = { git, rev = 4aa78031… }` (workspace `Cargo.toml`).
  syntect's unreleased `master` already migrated to the maintained **`yaml-rust2 0.10.4`** and is
  still package-version `5.3.0`, so the pin satisfies `tui-markdown ^5.3.0` with zero source edits.
- **Verified:** `cargo tree -i yaml-rust` → not present; `yaml-rust2 0.10.4` present; the
  `tui` `highlight-code` parity test (`test_code_blocks_rendered_with_highlighting`) still passes.
- **NOT ignored** in `.cargo/audit.toml` — if `yaml-rust 0.4` reappears it is a real regression.

### RUSTSEC-2025-0141 — `bincode 1.3.3` unmaintained — ⏸️ ACCEPTED-RISK (watch)

- **Why not fixable upgrade-only:** the advisory flags **all** bincode versions (1/2/3) —
  the crate was permanently frozen (`patched = []`, 2025-12-16). bincode 2.x/3.x are **not** a
  maintained successor; moving to them would not clear the advisory and would require
  regenerating syntect's embedded dumps in a new format.
- **Why it can't be dropped:** it enters ONLY via `syntect`'s bundled default syntax/theme set,
  which is structurally bincode-encoded (`default-syntaxes` → `dump-load` → `bincode`). The
  rusty-idd TUI's code-block highlighting needs the default syntaxes. No drop-in, no-bincode,
  maintained highlighter exists that preserves `tui-markdown`'s `highlight-code` integration
  (synoptic/syntastica/inkjet are not drop-in and/or pull C tree-sitter grammars).
- **Disposition:** ignored in `.cargo/audit.toml` (`RUSTSEC-2025-0141`) with this rationale.
- **Watch / exit condition:** upstream `trishume/syntect#623` (bincode-unmaintained) and `#694`
  (replace bincode), slated for **syntect 6.0.0**. When 6.0.0 ships and `tui-markdown` revs to it:
  update the `[patch] syntect` pin (or drop it), **remove the ignore**, and mark this FIXED.

## Not-yet-pursued (noted, not blocking)

- **`onig` / `onig_sys` (C dependency)** is pulled by syntect's default `regex-onig` backend.
  Not a `cargo audit` finding (it is maintained), but it is a C dep in a no-C-preferring tree.
  Removable by vendoring a syntect fork whose `default` uses `regex-fancy` (pure-Rust
  `fancy-regex`) — the heavier "host locally" option. Deferred; revisit with the syntect 6.0 bump.
- **`crates/spec` comrak footprint:** `crates/spec` uses comrak only for parse/emit (no
  `SyntectAdapter`), so `comrak = { version = "0.52", default-features = false }` would drop
  syntect from the spec path. Pure standalone-footprint hygiene; does not change the workspace
  audit (the `tui` path still pulls syntect). Deferred to keep the dep-fix PR surgical.
