# Code Intelligence

This project has code intelligence tools available via MCP (`kb_callers`, `kb_symbols`, etc.) and CLI (`git kb callers`, etc.). **Prefer MCP tools** — they support parallel calls and return structured JSON. Fall back to CLI via Bash if MCP is disconnected.

The daemon automatically re-indexes files on save via file watching (500ms debounce). No manual re-indexing needed during normal coding.

## Use Code Intelligence Instead of Grep

Do NOT use Grep or `grep` to find callers, usages, or definitions of functions/methods/types. Use code intelligence tools instead — they understand the AST, not just text matches.

| Instead of | Use |
|------------|-----|
| `Grep` for function callers | `kb_callers` — returns actual call sites from the call graph |
| `Grep` for function definitions | `kb_symbols` with `search:` — finds by name with signature and location |
| `Grep` to understand what a function calls | `kb_callees` — returns actual callees from the call graph |
| `Grep` to assess change impact | `kb_impact` with `file_path:` — transitive blast radius analysis |
| `Glob` + `Grep` to find dead code | `kb_dead_code` — finds symbols with zero callers |

Grep is still appropriate for searching config files, string literals, error messages, and non-code content.

## Before Modifying Functions

Before changing a function signature, renaming a symbol, or modifying a struct's fields, check callers:

```text
kb_callers with symbol: "<symbol_name>"
```

This shows every call site that would break. Use this to assess blast radius before making changes.

## When Exploring Unfamiliar Code

When you need to understand a module or file you haven't seen before, run these in parallel:

```text
kb_symbols with file_path: "<file_path>"     # List all symbols in a file
kb_callers with symbol: "<symbol_name>"      # Who calls this?
kb_callees with symbol: "<symbol_name>"      # What does this call?
```
