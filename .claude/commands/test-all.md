---
description: Run the full test suite across frontend and Rust core
allowed-tools: Bash
---

# Test All

Run every suite and report results honestly. Do not summarise a failure as a
pass, and do not skip a suite because an earlier one failed.

## Frontend

```bash
npm run typecheck
npm run lint
npm test
```

## Rust core

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

## Report

For each suite: pass/fail, and for failures the exact command and the failing
assertion. If `protocol.rs` or `filesystem.rs` changed in this diff and their
tests did not, call that out — those two files hold the security boundary and
changes there require tests (see CLAUDE.md).

Close with a single line: the number of suites passing out of the number run.
