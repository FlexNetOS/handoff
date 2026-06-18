RUST_BACKTRACE ?= full
RUST_LOG ?= trace

.DEFAULT_GOAL := build

build:
	cargo build --workspace

test:
	RUST_BACKTRACE=$(RUST_BACKTRACE) RUST_LOG=$(RUST_LOG) cargo test --workspace

fmt:
	cargo fmt -p hf -p ledger -p work-order

fmt-check:
	cargo fmt -p hf -p ledger -p work-order -- --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

release:
	cargo build --release -p hf

clean:
	cargo clean

# Install git hooks (commit-msg validates release-safe subjects; pre-push runs CI checks locally)
# Uses git rev-parse to handle worktrees and submodules correctly
install-hooks:
	@echo "Installing git hooks..."
	@chmod +x .githooks/commit-msg
	@chmod +x .githooks/pre-commit
	@chmod +x .githooks/pre-push
	@mkdir -p "$$(git rev-parse --git-path hooks)"
	@hooks_dir="$$(git rev-parse --git-path hooks)" && root="$$(git rev-parse --show-toplevel)" && rel="$$(python3 -c 'import os, sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$$root/.githooks/commit-msg" "$$hooks_dir")" && ln -sf "$$rel" "$$hooks_dir/commit-msg"
	@hooks_dir="$$(git rev-parse --git-path hooks)" && root="$$(git rev-parse --show-toplevel)" && rel="$$(python3 -c 'import os, sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$$root/.githooks/pre-commit" "$$hooks_dir")" && ln -sf "$$rel" "$$hooks_dir/pre-commit"
	@hooks_dir="$$(git rev-parse --git-path hooks)" && root="$$(git rev-parse --show-toplevel)" && rel="$$(python3 -c 'import os, sys; print(os.path.relpath(sys.argv[1], sys.argv[2]))' "$$root/.githooks/pre-push" "$$hooks_dir")" && ln -sf "$$rel" "$$hooks_dir/pre-push"
	@echo "Commit-msg, pre-commit, and pre-push hooks installed."

.PHONY: build test fmt fmt-check clippy release clean install-hooks
