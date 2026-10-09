---
description: Scaffold a new Rust core module with wiring, tests, and error type
argument-hint: <module-name>
allowed-tools: Read, Write, Edit, Glob, Grep
---

# Bootstrap a Core Module

Create a new module in the Rust core named `$1`.

## Before writing

1. Confirm `$1` maps to a real component in SRS §12 or §10. If it doesn't, stop
   and say so — unimplemented scope is not a module.
2. Check whether an existing module already owns this responsibility. Prefer
   extending over adding.

## Create

`src-tauri/src/$1.rs` with:

- a module doc comment stating what it owns and which SRS requirements it satisfies
- a `thiserror`-derived `Error` enum covering its failure modes
- public functions returning `Result<_, Error>` — no panics on I/O paths
- a `#[cfg(test)] mod tests` with at least one test per public function,
  including the failure paths

## Wire it in

- Declare the module in `src-tauri/src/lib.rs`.
- If it needs to reach the UI, add commands in `commands.rs` and register them
  in the `invoke_handler` list, then mirror the types in `src/types/index.ts`.
- If it emits progress or state, define the event name as a const and document it
  in `docs/api-reference.md`.

## Conventions to follow

- Async via Tokio. Filesystem work goes through `spawn_blocking`.
- Comment *why*, not *what*. Cite SRS requirement IDs for security-relevant choices.

## Finish

Run `cargo fmt`, `cargo clippy -- -D warnings`, and `cargo test` in `src-tauri/`.
Report what you created and the test count.
