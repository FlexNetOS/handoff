# Ollama Provider Discipline

When operating in this workspace under the Ollama provider:

1. **Prioritize Local Knowledge**: Use `hf status` and `hf resume` to understand the workspace instead of asking the user for basic info.
2. **Compress Output**: Always use `rtk` (if available) when running commands that produce large logs (e.g., `cargo test --workspace | rtk`).
3. **Chunk Large Tasks**: Break down multi-crate refactors into smaller, verifiable steps to accommodate local model context limits.
4. **Tool Verification**: Confirm tool availability (like `hf`, `rtk`, `cargo`) before suggesting complex workflows.
