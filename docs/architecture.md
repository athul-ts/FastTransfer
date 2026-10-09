# Architecture

The complete requirements baseline is [`FastTransfer_SRS_v3.0.md`](../FastTransfer_SRS_v3.0.md).
This document explains how the code is laid out and why.

## The shape of the system

```text
┌──────────────────────────────────────────────┐
│  React UI  (src/)                            │
│  Renders. Holds presentation state only.     │
└───────────────────┬──────────────────────────┘
                    │  Tauri IPC (commands + events)
                    ▼
┌──────────────────────────────────────────────┐
│  Rust core  (src-tauri/src/)                 │
│  Owns networking, I/O, crypto, state.        │
└──────────────────────────────────────────────┘
```

**The frontend never implements transfer logic.** It calls a command, or listens
for an event, and renders the result. If you find yourself writing a loop over
chunks in TypeScript, stop — that belongs in the core (SRS §11.1).

## Why the split is where it is

The transfer engine needs zero-copy I/O, bounded buffers, and control over
concurrency. A webview cannot provide those, and the Tauri IPC boundary is a
serialization cost you do not want on the hot path. So the boundary is drawn at
"one command in, one event stream out" — no per-chunk traffic across IPC.

## Core modules

| Module | Owns | SRS |
|---|---|---|
| `link.rs` | What a plugged cable can actually do | §4, §17 |
| `discovery.rs` | Finding peers, including with no router | §13 |
| `transport.rs` | Interface enumeration, ranking, fallback | §14, §16 |
| `protocol.rs` | Framing, messages, negotiation | §18, §19, §58, §59 |
| `transfer.rs` | Chunking, state machine, resume, backpressure | §20–§22, §27–§29 |
| `security.rs` | Identity, pairing, trust | §12, §15, §34–§36 |
| `filesystem.rs` | I/O and the path-safety boundary | §23–§26, §91 |
| `commands.rs` | The IPC surface | §79 |
| `error.rs` | Errors mapped to user-facing meaning | §95 |

## The two invariants that matter

Everything else in this codebase is replaceable. These two are not.

### 1. Path safety — `filesystem::safe_join`

> For any destination root `R` and any wire-supplied path `P`, if
> `safe_join(R, P)` returns `Ok(path)`, then `path` is inside `R`.

`safe_join` is purely lexical: it rejects `..`, absolute paths, Windows drive
and UNC prefixes, reserved device names, and trailing dots. It **cannot** know
that `root/link` is a symlink to `/etc`, so `verify_within` runs after it, once
the destination exists, and canonicalises the deepest existing ancestor.

A false rejection costs the user a rename. A false acceptance writes outside the
destination folder. The function is deliberately paranoid.

### 2. Chunk atomicity — `transfer::ResumeManifest`

> A chunk is complete only when the receiver has read its declared length *and*
> its trailing digest matched. A partial chunk is never persisted.

Resume depends entirely on this. If a partial chunk is ever recorded as
complete, the transfer "succeeds" with a hole in it and the SHA-256 check at the
end is the only thing that catches it — and only if verification is on.

Corollary: the receiver's manifest is remote input. `ResumeManifest::validate`
checks every claimed chunk index against the sender's own view of the file
sizes before any of it is believed.

## Data flow: a transfer

```text
1. link.rs       classify the cable           → LinkKind
2. discovery.rs  find the peer                → Peer
3. security.rs   pair, get user approval      → TrustedDevice
4. transport.rs  rank and select a path       → TransportCandidate   ← over InterfaceKind
5. protocol.rs   HELLO / AUTH / REQUEST       → Capabilities, Accept
6. transfer.rs   prepare(entries, root)       → Vec<PreparedItem>   ← paths proven here
7. transfer.rs   chunk_plan + pipeline        → ChunkBuffer stream  ← backpressure here
8. filesystem.rs write each chunk             → files on disk
9. transfer.rs   verify SHA-256               → Verified | VerificationFailed
```

### `LinkKind` and `InterfaceKind` are not the same thing

`LinkKind` answers "what can this plugged **cable** do?" — it has no Ethernet or
Wi-Fi variant, because neither is a cable. `InterfaceKind` answers "what is this
**interface**?" and does cover them.

Transport selection reads `InterfaceKind`. Deriving the transport class from
`LinkKind` instead meant `TransportKind::from_link` could only ever return
`Thunderbolt`, so the Ethernet and Wi-Fi fallback required by SRS §16 was
unreachable — the app would have silently been cable-only. If you add a medium,
add it to `InterfaceKind` and map it in `TransportKind::from_interface`.

Step 6 is the choke point. Every path from the peer is proved safe there, and
the write path later assumes that work is done. Adding a new write path that
skips `prepare` is a security regression.

Step 7's channel is bounded (`PIPELINE_DEPTH`). That bound *is* the backpressure:
when the destination disk is slower than the link, the channel fills, the reader
stops pulling, and TCP's window closes. An unbounded channel here turns a fast
cable into an OOM.

## Backpressure, concretely

```text
cable reader ──▶ [ bounded channel, depth 8 ] ──▶ disk writer
                        │
                        └── when full, the reader awaits,
                            so nothing is buffered beyond 8 chunks
```

With a 4 MiB chunk size that caps in-flight data at ~32 MiB per stream,
regardless of file size. A 100 GB file uses the same memory as a 1 GB one
(SRS §30).

## Transport independence

The engine talks to `Box<dyn Transport>`, never to TCP. Adding QUIC is a new
implementation of the trait, not a change to the engine (SRS §11.2).

```text
Transfer Engine
       │
       ▼
  Transport (trait)
       │
 ┌─────┼─────┐
 ▼     ▼     ▼
TB   LAN   Wi-Fi
```

## State ownership

| State | Owner | Frontend's copy |
|---|---|---|
| Peers | `discovery.rs` | cache, refreshed by events |
| Link status | `link.rs` | cache, refreshed by `link_changed` |
| Transfers | `transfer.rs` | cache, refreshed by `transfer_*` |
| Trust | `security.rs` | cache, refreshed on demand |

The frontend never computes authoritative transfer state. If the UI and the core
disagree, the core is right (SRS §80).

## Platform notes

**Thunderbolt networking is per-OS and uneven.** macOS uses the Thunderbolt
Bridge; Windows needs the Thunderbolt Network Service; Linux exposes it through
`boltd`. `link.rs` reports `LinkKind::None` until those probes are implemented,
because a wrong `THUNDERBOLT` claim is worse than an honest `None`.

**Direct cable means no DHCP.** Both peers end up on link-local addresses
(`169.254.0.0/16`), and there is typically no mDNS responder. That is why
`discovery.rs` has a link-local probe alongside mDNS — the router-free path is
the product's primary use case, not an edge case (SRS §73).

## Testing

- Rust unit tests live beside the code. `protocol.rs` and `filesystem.rs` are the
  security boundary — changes there require tests in the same commit.
- Frontend logic is tested with Vitest. Components are tested only where they
  encode a rule (the error notice's remedy, the fraction guard).
- Integration tests use a loopback transport. Real-cable verification is manual,
  against the matrix in SRS §87–§88.
