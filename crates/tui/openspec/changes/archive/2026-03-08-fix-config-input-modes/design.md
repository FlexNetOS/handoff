## Context

The config screen (`Screen::Config`) currently treats all keypresses as text input when the Command field is focused. Shortcuts like `S` (save) and `D` (reset defaults) only work when the Prompt field is focused, because they are guarded with `if *focused_field != ConfigField::Command`. Since the screen starts with Command focused, users must Tab away before any shortcut is accessible.

Additionally, a stale `openspec/tui-config.yaml` with test data overrides the defaults.

## Goals / Non-Goals

**Goals:**
- Separate navigation from text editing via two distinct input modes
- Make `S`, `D`, `Esc` shortcuts always reachable from navigation mode
- Provide clear visual feedback for which mode is active
- Clean up stale config file

**Non-Goals:**
- Inline editing for the Prompt field (continues to use `$EDITOR`)
- Adding new config fields beyond command and prompt
- Vim-style modal editing beyond simple enter/escape toggling

## Decisions

### 1. Add `editing: bool` flag to `Screen::Config`

Instead of creating a separate enum or new screen variant, add an `editing: bool` field to `Screen::Config`. When `false`, the screen is in navigation mode. When `true` and `focused_field == Command`, inline editing is active.

**Rationale**: Minimal change — avoids restructuring the existing enum. The boolean is sufficient because only the Command field has an edit mode (Prompt uses `$EDITOR`).

**Alternative considered**: A `ConfigMode` enum (`Navigation`, `EditingCommand`) — cleaner semantically but adds a new type for a simple toggle. Not worth the complexity.

### 2. Key routing by mode

```
Navigation mode:
  Tab/BackTab  → switch focused field
  Enter        → activate edit mode (Command) or open $EDITOR (Prompt)
  S            → save and exit
  D            → reset to defaults
  Esc          → cancel and exit config screen

Edit mode (Command field only):
  Char(c)      → insert character
  Backspace    → delete before cursor
  Delete       → delete at cursor
  Left/Right   → move cursor
  Home/End     → jump to start/end
  Esc          → return to navigation mode
  Enter        → return to navigation mode (confirm edit)
```

**Rationale**: `Esc` in navigation mode exits the config screen (existing behavior). `Esc` in edit mode returns to navigation. This is intuitive and matches common TUI patterns.

### 3. Visual states

- **Navigation, field focused**: Yellow border, no cursor shown
- **Navigation, field unfocused**: DarkGray border
- **Edit mode active**: Yellow border + visible block cursor in text

This reuses existing styling with the cursor as the only differentiator between focused and editing.

## Risks / Trade-offs

- [Extra Enter press required to start editing] → Acceptable trade-off for reliable shortcut access. The previous behavior was broken for shortcuts.
- [Users might expect typing to work immediately] → Keybinding hints at the bottom will show `[Enter] Edit` in navigation mode to guide users.
