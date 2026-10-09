# CLAUDE.md

FastTransfer — cable-first direct file transfer between two computers.

## What this project is

A Tauri 2 desktop application (React + TypeScript + Vite frontend, Rust core) that
moves files directly between two machines with **no router, no DHCP, and no Internet**,
using the fastest path the two machines actually support — **Thunderbolt 3 / USB-C**
where both endpoints expose a Thunderbolt-backed network interface, and direct
Ethernet or Wi-Fi where they do not.

Primary objective: transfer over the best available direct path, and report honestly
which path that is. Cable-speed transfer is the point of the product, but it is a
**hardware-conditional** capability (§5.2), not a guaranteed one — it cannot be
assumed, and it does not hold on the reference hardware (§116.1.1). Never claim a
capability the hardware does not have.

## Read these first

- `FastTransfer_SRS_v3.0.md` — the requirements baseline. Authoritative for scope.
- `docs/architecture.md` — component map and data flow.
- `docs/api-reference.md` — the Tauri command/event contract.

## Repository layout

```text
src/                  React frontend (UI only, no transfer logic)
  components/         feature-grouped UI
  services/           typed wrappers around Tauri IPC
  types/              shared TypeScript types mirroring the Rust model
  utils/              logger, validators, helpers
src-tauri/            Rust core — owns all networking, I/O, crypto
  src/
    lib.rs            Tauri app setup and command registration
    commands.rs       the IPC surface exposed to the frontend
    protocol.rs       wire framing and message types
    link.rs           cable classification (THUNDERBOLT / USB_DATA_ONLY / ...)
    discovery.rs      mDNS + router-free link-local fallback
    transport.rs      interface enumeration, ranking, fallback
    transfer.rs       chunked streaming, resume, backpressure
    security.rs       device identity, pairing, session crypto
    filesystem.rs     file I/O, path safety, cross-platform compat
    error.rs          error types mapped to user-facing messages
tests/                frontend tests (unit / integration / e2e)
docs/                 architecture, API reference, onboarding
```

## Tech stack

Frontend: React 18, TypeScript (strict), Vite 6, Tailwind CSS, shadcn/ui
Desktop: Tauri 2
Core: Rust 2021
Protocol: TCP (length-prefixed JSON messages)
Integrity: SHA-256
Crypto: established libraries only — Noise or TLS 1.3. **Never roll your own.**

## Conventions

**Rust**
- `snake_case` functions, `PascalCase` types, `SCREAMING_SNAKE_CASE` consts.
- Every fallible public function returns `Result<T, Error>` — no panics on I/O paths.
- Use `thiserror` for error types; map to user-facing strings in `error.rs`, not at the call site.
- Async via Tokio. No blocking I/O on the async runtime — use `tokio::task::spawn_blocking` for filesystem work.

**TypeScript**
- Strict mode on. No `any` — use `unknown` and narrow.
- All IPC goes through `src/services/`; components never call `invoke()` directly.
- Types in `src/types/index.ts` must mirror the Rust structs. When you change a Rust
  type that crosses the IPC boundary, update the TS mirror in the same change.
- Functional components only. No class components.

**Comments**
- Comment *why*, not *what*. Match the density of the surrounding file.
- Security-relevant decisions get a comment citing the SRS requirement ID.

## Testing

- Rust: `cargo test` in `src-tauri/`. Unit tests live beside the code in `#[cfg(test)]` modules.
- Frontend: `npm test` (Vitest). Pure logic in `utils/` and `services/` must be tested.
- Any change to `protocol.rs` or `filesystem.rs` requires a test. These two hold the
  security boundary.
- Integration tests for transfer/resume use a loopback transport, not a real cable.
- Real-cable verification is manual, on the matrix in SRS §87.

## Security rules — non-negotiable

1. **Never trust remote metadata.** Filenames, paths, sizes, chunk indices, and hashes
   arriving over the wire are untrusted input. Validate before use.
2. **Path traversal is a critical bug.** Every destination path must be proven to be
   inside the chosen destination directory before any write. See `filesystem::safe_join`.
3. **No custom cryptography.** Use established crates. If you need a primitive that
   isn't available, stop and ask — do not implement it.
4. **Discovery never implies authorization.** A peer found on the cable is a stranger
   until the user approves it.
5. **Logs never contain file contents or key material.**

## Error handling patterns

Every user-facing error must answer three questions: what happened, why, and what the
user can do about it. Map errors at the boundary in `error.rs`:

```rust
// Bad — leaks internals, tells the user nothing
return Err("ECONNRESET".into());

// Good — actionable, and carries the resume affordance
return Err(Error::ConnectionLost { transferred: 72_400_000_000, resumable: true });
```

## Git workflow

- Branch from `main`: `feat/…`, `fix/…`, `chore/…`.
- Conventional Commits: `feat(transfer): add chunk-level resume`.
- One logical change per commit. Tests ship with the code they cover.
- Never commit secrets. `.env` is gitignored; `.env.example` documents the shape.

See `docs/architecture.md` for the complete architecture specification.
