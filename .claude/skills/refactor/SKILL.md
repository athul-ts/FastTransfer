---
name: refactor
description: Refactor FastTransfer code without changing behaviour — extract modules, break up large functions, tighten types, and remove duplication. Use when the user asks to clean up, simplify, restructure, or "make this nicer", and when a file has grown past the point where it reads clearly.
---

# Refactor

Refactoring changes structure, never behaviour. If behaviour changes, that is a
bug fix or a feature — do it in a separate commit and say so.

## Before touching anything

1. **Establish the safety net.** Run the tests for the code you are about to
   change. If they pass now, they must still pass after. If they don't exist,
   write them first — a refactor without tests is a rewrite with extra steps.
2. **Confirm the boundary is untouched.** `protocol.rs`, `filesystem.rs`, and
   `security.rs` are the hostile-peer boundary. Refactoring them is allowed but
   requires a test run before and after, and any change to control flow there
   needs a re-read as if it were new code.

## What to look for

**Altitude.** This is the usual problem in this codebase. Signs a function is at
the wrong altitude:

- It both parses wire input and makes policy decisions. Split: parse into a typed
  struct, then decide.
- It mixes async orchestration with blocking filesystem work. Move the blocking
  work into `spawn_blocking` behind a function that reads as one operation.
- It is named after what it does mechanically (`process_chunks`) rather than what
  it means (`apply_resume_manifest`).

**Duplication across transports.** Every transport implements the same interface
(§11.2). If Thunderbolt, Ethernet, and Wi-Fi paths share logic, that logic belongs
in the trait's default method or a shared helper — never copy-pasted three times.

**Error types.** Errors should be mapped to user-facing meaning in one place
(`error.rs`). Scattered `map_err` closures that build strings are a smell.

**Types that lie.** `Option` used where a state machine belongs, `bool` flags
that are only ever set together, `String` where an enum is meant. Replacing these
usually deletes code rather than adding it.

**The TS/Rust mirror.** If refactoring a Rust struct crossing the IPC boundary,
update `src/types/index.ts` in the same change. A drifted mirror compiles and
then fails at runtime.

## What not to do

- Do not reformat unrelated code. A refactor diff should be readable as *only*
  the structural change.
- Do not rename public API without flagging it — it may break the frontend.
- Do not "simplify" the path-safety or crypto code. Those look redundant on
  purpose; the redundancy is defence in depth.
- Do not add an abstraction for two call sites. Wait for three.

## After

Report: what moved, what was deleted, and the before/after test counts. If the
diff is net-positive in lines, justify it — a refactor that adds code needs a
reason.
