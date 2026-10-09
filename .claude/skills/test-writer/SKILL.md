---
name: test-writer
description: Write tests for FastTransfer — Rust unit tests in the core, Vitest tests for frontend logic, and integration tests for transfer and resume. Use when adding a feature, fixing a bug, or when the user asks for tests, coverage, or "make sure this works".
---

# Test Writer

## Where tests live

| Target | Location | Runner |
|---|---|---|
| Rust core | `#[cfg(test)] mod tests` in the same file | `cargo test` |
| Rust integration | `src-tauri/tests/*.rs` | `cargo test` |
| Frontend logic | `tests/unit/*.test.ts` | Vitest |
| Frontend components | `tests/unit/*.test.tsx` | Vitest + Testing Library |
| Transfer/resume flows | `tests/integration/*.test.ts` | Vitest |

`protocol.rs` and `filesystem.rs` hold the security boundary. Any change to
either requires tests in the same commit — no exceptions.

## What to test

Test behaviour, not implementation. For each public function, the interesting
cases are usually the failure modes, not the happy path.

**Protocol (`protocol.rs`)**
- round trip: encode then decode returns the original
- a payload one byte over `MAX_MESSAGE_SIZE` is rejected before allocation
- a truncated frame (header promises more than the stream delivered) yields a
  clean error, not a hang or a panic
- an unknown message type is rejected, not ignored

**Path safety (`filesystem::safe_join`)**
This is the highest-value test surface in the repository. Cover:
- `../` traversal in the middle and at the end
- absolute paths supplied where a relative path is expected
- Windows: backslash separators, drive letters (`C:\`), UNC paths (`\\?\`)
- Windows reserved device names: `CON`, `PRN`, `AUX`, `NUL`, `COM1`–`COM9`, `LPT1`–`LPT9`
- embedded NUL bytes
- Unicode normalization: NFC vs NFD forms of the same filename must not produce
  two files (SRS FR-NAME-001)
- a symlink inside the destination pointing outside it

**Resume (`transfer.rs`)**
- interrupt after chunk *N*, resume, assert only chunks `> N` are re-sent
- a partial chunk at the interruption point is re-sent in full
- receiver reporting a forged "completed" set does not cause data loss

**State machine**
- `COMPLETED` is unreachable unless verification passed
- cancel during `VERIFYING` behaves sanely
- every transition that the UI can trigger has a defined result

**Frontend (`src/utils`, `src/services`)**
- formatting and parsing of byte counts and durations
- error mapping: a Rust error tag renders the intended user-facing message
- IPC wrappers build the right argument shape (mock `invoke`)

## Rules

- One assertion concept per test. A test that checks five things reports one
  failure and hides four.
- Name tests as `fn <subject>_<condition>_<expected>()`, e.g.
  `safe_join_rejects_parent_traversal`.
- No network, no real filesystem in unit tests — use `tempfile` for the Rust
  side and in-memory mocks elsewhere.
- Never assert on a wall-clock duration or a throughput number; they are flaky
  by construction.
- Do not test private helpers directly. If it needs a test, it wants to be `pub`
  or it wants to be tested through its caller.

## After writing

Run the suite. Report the count of passing tests and the exact output of any
failure — do not describe a failing test as "mostly working".
