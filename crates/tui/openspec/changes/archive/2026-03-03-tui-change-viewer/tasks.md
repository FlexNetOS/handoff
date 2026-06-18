## 1. Project Setup

- [x] 1.1 Add dependencies to Cargo.toml: ratatui, crossterm, serde, serde_json
- [x] 1.2 Set up terminal initialization and cleanup (raw mode, alternate screen) with restore-on-panic

## 2. Data Layer

- [x] 2.1 Define data types for openspec CLI JSON output (change list, artifact status)
- [x] 2.2 Implement function to run `openspec list --json` and parse the result
- [x] 2.3 Implement function to run `openspec status --change <name> --json` and parse the result
- [x] 2.4 Implement function to read artifact file content from disk
- [x] 2.5 Implement function to discover spec sub-items by listing the `specs/` subdirectory of a change

## 3. App State and Screen Model

- [x] 3.1 Define screen enum (ChangeList, ArtifactMenu, ArtifactView) with per-screen state
- [x] 3.2 Implement screen transitions (Enter to push, Esc to pop, q to quit)

## 4. Change List Screen

- [x] 4.1 Render the change list using ratatui List widget
- [x] 4.2 Handle keyboard input for navigation (up/down/j/k) and selection (Enter)
- [x] 4.3 Handle empty state (no active changes message)
- [x] 4.4 Handle openspec CLI not found error

## 5. Artifact Menu Screen

- [x] 5.1 Render the artifact list (Proposal, Design, Tasks, Specs) with greyed-out styling for unavailable items
- [x] 5.2 Expand Specs item to show individual spec sub-items
- [x] 5.3 Handle keyboard input: navigation, Enter (only on available items), Esc to go back

## 6. Artifact Content View Screen

- [x] 6.1 Render plain text content in a scrollable view
- [x] 6.2 Handle keyboard input for scrolling (up/down/j/k) and Esc to go back

## 7. Testing

- [x] 7.1 Add tests for JSON parsing of openspec CLI output
- [x] 7.2 Add tests for screen state transitions
- [x] 7.3 Add tests for artifact availability logic (done vs greyed out)
