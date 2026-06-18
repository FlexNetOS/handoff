# Destructive Command Safety

Destructive commands can lose work or break continuity. Pause before running them.

## Before Destructive Operations

1. **Make a checkpoint first**
   ```
   hf checkpoint <id> "before risky change"
   ```

2. **Preview with --dry-run**
   ```
   git clean -nd
   ```

3. **Target precisely** — avoid blanket operations
   ```
   git checkout -- <file>
   ```

## Blocked by `agent guard`

These commands trigger PreToolUse denial:
- `git push --force` (use `--force-with-lease` instead)
- `git reset --hard` (use `git checkout -- <file>` or `git restore` instead)
- `git clean -fd` (dangerous file removal)
- `rm -rf` on repo roots or `.handoff/`, `.github/`, `.claude/` paths

## Recovery

If something goes wrong:
```
git reflog
hf drift
hf doctor
```
