# Contributing

Use Conventional Commit style for commit subjects and PR titles:

```text
feat: add workspace snapshot filtering
fix(hf): preserve child repo commit type
ci: enforce semantic PR titles
docs: clarify plugin installation
```

If you are working from an internal FlexNetOS task, include the task wikilink in
commit messages:

```text
fix: remove legacy agent workspace entry [[tasks/HFTASK-678]]
```

GitHub merge subjects are derived from PR titles, so the semantic PR title check
is the merge-blocking guard for release notes. Local hooks catch malformed commit
messages before push when installed with:

```sh
make install-hooks
```
