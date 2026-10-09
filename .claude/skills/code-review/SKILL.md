---
name: code-review
description: Review FastTransfer code changes for correctness, security-boundary violations, and IPC drift. Use when reviewing a diff, a pull request, or a change to the Rust core or the React frontend, and when the user asks for a review, a second opinion, or "does this look right".
---

# Code Review

You are reviewing a change to FastTransfer, a cable-first file transfer app. The
thing that makes this codebase different from a typical app is that **it processes
input from a peer that may be hostile**, and it does so in a language (Rust) where
the tempting shortcuts are memory-unsafe or path-unsafe.

## Priority order

Review in this order. Do not reorder — a security finding outranks ten style nits.

### 1. The hostile-peer boundary

Anything that consumes remote input lives in `protocol.rs`, `discovery.rs`,
`transfer.rs`, and the receive path of `filesystem.rs`. For every change there:

- **Path safety.** Every destination path must be proven inside the destination
  root. The check must happen on the *final* resolved path, after normalization,
  symlink resolution, and case folding. A check performed on the raw input, or on
  a path that is later joined again, is a vulnerability. Cite SRS §91.
- **Allocation from untrusted sizes.** Is a buffer sized from a wire-supplied
  length before that length is validated against `MAX_MESSAGE_SIZE`? That is a
  remote OOM. Cite SRS §93.
- **Chunk indices and counts.** A peer claiming chunk index `u64::MAX` or a count
  of 10^12 must be rejected, not multiplied.
- **Time-of-check to time-of-use.** Between validating a destination and writing
  to it, could the path have changed? On the receive side this matters for
  symlink swaps.

### 2. Correctness of the transfer state machine

- Can a transfer reach `COMPLETED` without `VERIFYING` having passed? That
  inverts the product's core promise.
- Is a partial chunk ever persisted as complete? SRS FR-CHUNK-001 makes a chunk
  the atomic unit of resume.
- On resume, does the receiver's claim of "completed chunks" get trusted blindly?
  It is remote input.
- Backpressure: is there a bounded channel somewhere between network read and
  disk write? An unbounded one is a memory leak that shows up only on big files.

### 3. IPC boundary consistency

Rust structs that cross the Tauri boundary must have a mirror in
`src/types/index.ts`. If a field was renamed, added, or made optional and the TS
side did not change, the mismatch fails at runtime, not compile time. Flag it.

### 4. Error quality

Every user-visible error answers: what happened, why, what now. `Err("ECONNRESET")`
reaching the UI is a bug (SRS §95). Check `error.rs` mapping, not the call site.

### 5. Scope

Is this in the SRS? If it implements something in §6 (Non-Goals) or outside the
MVP in §98, say so and stop reviewing it as if it belonged.

## How to report

- Rank by severity: **Critical** (security, data loss, silent corruption) →
  **High** (correctness) → **Medium** (robustness) → **Low** (clarity).
- Every finding needs: `file:line`, the defect in one sentence, and a concrete
  failure scenario — inputs and resulting wrong behaviour.
- If a severity level is empty, say so in one line. Do not pad the report.
- Never restate the diff back to the author.
