# rusty-idd-tui (formerly `openspec-tui`)

A terminal UI for browsing and implementing [OpenSpec](https://github.com/Fission-AI/OpenSpec) changes. Built with Rust using [ratatui](https://github.com/ratatui/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm).

This crate is now unified into the **`rusty-idd`** CLI.

## Quick Start

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) toolchain (edition 2024)
- Unified **`rusty-idd`** CLI installed (or run from source)

### Using Nix (recommended)

```sh
nix develop
cargo run --bin rusty-idd -- tui
```

### Manual Setup

```sh
cargo build --bin rusty-idd
./target/debug/rusty-idd tui
```

Launch the TUI from a directory that contains an `openspec/` folder with changes:

```sh
rusty-idd tui
```

## Features

### Browsing Changes

- Switch between **Active** and **Archived** tabs with `h`/`l`
- Change list shows task progress (completed/total) per change
- Select a change to view its artifacts (Proposal, Design, Tasks, Specs)

### Implementation Runner

- Start implementation on a change by pressing `Enter` on the Artifact Menu
- Re-run implementation with `R`, stop a running implementation with `S`
- Stall detection: automatically detects when implementation stalls (300s timeout)
- Post-implementation hooks for automated follow-up actions

### Batch Mode

- Run all changes at once with `A` (opens multi-select screen)
- Toggle individual changes with `Space`, confirm selection with `Enter`
- Respects dependency ordering during batch execution
- Run-finished notification via configurable command

### Dependencies

- View and manage per-change dependencies in the Dependency View (`D` from Artifact Menu)
- Add dependencies from available changes (`A` in Dependency View)
- Visualize the full dependency graph with `G`
- Toggle run mode between Normal and Apply with `M`

### Interactive Tool Launch

- Launch an external tool (e.g. `claude`) directly from the change list with `I`
- Configurable interactive command

### Configuration

- Built-in TUI config editor accessible with `C`
- Configurable fields: Command, Prompt, Post-Implementation Prompt, Interactive Command, Run Finished Command
- Config stored in `openspec/tui-config.yaml`

### Log Viewing

- Live log output during implementation (status bar)
- View full implementation logs after completion with `L`

## Keybindings

### Global

| Key | Action |
|-----|--------|
| `q` | Quit |
| `S` | Stop running implementation |
| `r` | Refresh current view |

### Change List

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection up/down |
| `Enter` | Open selected change |
| `h` / `l` | Switch Active/Archived tab |
| `C` | Open config editor |
| `G` | Show dependency graph |
| `A` | Run all (batch mode) |
| `I` | Launch interactive tool |

### Artifact Menu

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection up/down |
| `Enter` | Open artifact / start implementation |
| `L` | View implementation log |
| `R` | Re-run implementation |
| `D` | Open dependency view |
| `C` | Open config editor |
| `Esc` | Back to change list |

### Artifact View

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll up/down |
| `C` | Open config editor |
| `Esc` | Back |

### Dependency View

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection up/down |
| `D` | Remove selected dependency |
| `M` | Toggle run mode (Normal/Apply) |
| `A` | Add dependency |
| `Esc` | Back |

### Dependency Graph

| Key | Action |
|-----|--------|
| `j` / `k` | Scroll up/down |
| `Esc` | Back |

### Run All Selection

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection up/down |
| `Space` | Toggle change inclusion |
| `Enter` | Start batch run |
| `Esc` | Back |

### Config Editor

| Key | Action |
|-----|--------|
| `Tab` / `BackTab` | Next/previous field |
| `Enter` | Edit selected field |
| `S` | Save config |
| `D` | Reset to defaults |
| `X` / `Esc` | Cancel and go back |

## License

MIT — see [LICENSE](LICENSE).
