# F04 — Session establishment and wire protocol

| | |
|---|---|
| **Status** | Approved 2026-11-09 — In Progress — T criteria pass except AC-F04-13 (blocked on F08) |
| **SRS phase** | 4 (§108) |
| **SRS sources** | §18 (18.1–18.4), §19 (FR-SESS-001…004), §48, §58, §59, §90, §92, §93 |
| **Depends on** | F02, F08 (for authentication) |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

A bounded, versioned, length-prefixed protocol over TCP that two peers use to
agree on capabilities, ask permission, and move chunks — and that a hostile peer
cannot use to crash, stall or exhaust the receiver.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| §18.1 | First implementation uses TCP | shall | VALID | Implemented (F02 `TcpTransport`) |
| §18.3 | Negotiate version, features, chunk size, compression, resume, checksum, transports | shall | VALID | Implemented (`Capabilities::intersect`) |
| §18.4 | Length-prefixed, bounded (16 MiB); unknown types rejected; framing untrusted | shall | VALID | Implemented (`read_message`, serde rejects unknown `type`) |
| FR-SESS-001 | Versioned protocol | must | VALID | Implemented (`PROTOCOL_VERSION`, `negotiate`) |
| FR-SESS-002 | Explicit role model; deterministic tie-break | shall | **VALID WITH CHANGES** — finding 1 | Not started |
| FR-SESS-003 | Reject incompatible versions safely, clear message on both sides | shall | VALID | Partial: local rejection only; nothing tells the peer |
| FR-SESS-004 | Authenticate before any file data | must | VALID | **Not started — blocked on F08** |
| §48 | Receiver sees an incoming request and can reject | must | VALID | Not started |
| §92 | Validate every remote field | must | VALID | Partial: framing, manifest and resume validation exist; chunk-header validation not yet |

## 3. Validity findings

1. **FR-SESS-002 mixes pairing and transfer roles.** "The peer that initiates the
   pairing/connection request becomes the sender." After pairing, either side may
   send, so the pairing initiator cannot fix the role. If every transfer opens its
   own TCP connection, the connecting side is the sender and simultaneous
   initiation produces two independent sessions — no tie-break is needed. → CR-05.
2. **The `Hello`/`HelloAck` nonce exchange is a hand-built handshake shape.** It
   echoes nonces "to reject replayed handshakes" (`protocol.rs`), which is
   authentication logic outside an established library — what NFR-SEC-009
   forbids. Today it authenticates nothing. Once F08 lands, capability exchange
   must run *inside* the Noise/TLS session, and the nonces go away. → D-F04-1.
3. **The SRS §79 IPC list has no way to answer an incoming request.** §48 requires
   Accept/Reject, but §79 lists no `incoming_transfer` event and no
   `accept_transfer`/`reject_transfer` command. → CR-06.
4. **No minimum chunk size.** The negotiated size is the minimum of both peers;
   zero is now refused, but a peer can still advertise 1 byte. A 100 GB file would
   then plan 10¹¹ chunks. → CR-17, D-F04-3.
5. **One `TransferRequest` holds the whole manifest**, and a message is capped at
   16 MiB. At roughly 120 bytes per JSON entry, that fits about 130 000 entries.
   §64 asks for 100 000 small files and §63 for 1 TB+ collections. Fine for now,
   but it is a hard ceiling. → CR-18, D-F04-4.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F04-1 | Where does authentication sit? | Noise `XX` (F08) runs first on the raw TCP stream. `Hello`/`HelloAck` (capabilities only, no nonces) and everything after run encrypted inside it. Until F08, sessions are plaintext and the build is labelled non-conformant to FR-SESS-004/NFR-SEC-001. | Accepted 2026-11-09 |
| D-F04-2 | Message sequence | `Hello` → `HelloAck` → `TransferRequest` → (receiver asks the user) → `Accept{completed_chunks}` or `Reject` → for each missing chunk: `ChunkHeader`, raw bytes, `ChunkFooter{sha256}` → `TransferComplete{digests}` → `Verified` or `VerificationFailed`. `Error{code}` may be sent by either side at any point; `code: "cancelled"` means the user cancelled. | Accepted 2026-11-09 |
| D-F04-3 | Chunk size bounds | Default 4 MiB, minimum 64 KiB, maximum 16 MiB (one message). Negotiation outside the bounds is refused. | Accepted 2026-11-09 |
| D-F04-4 | Manifest ceiling | Keep one message and a 100 000-entry cap for v1 (`MAX_MANIFEST_ENTRIES`); refuse larger sets with a remedy ("send as an archive or in parts"). Paged manifests are a v1.1 change with a minor version bump. | Accepted 2026-11-09 |
| D-F04-5 | Timeouts | Handshake 10 s; waiting for the user's decision 120 s, then reject; idle data 30 s, then `INTERRUPTED`; no timeout while the receiver verifies. | Accepted 2026-11-09 |
| D-F04-6 | Version mismatch | Before closing, send `Error{code: "versionMismatch", detail: "<local>"}` so both sides can render FR-SESS-003's message. | Accepted 2026-11-09 |
| D-F04-7 | Remote strings | Display name ≤ 64 chars, transfer id must match `filesystem::is_valid_transfer_id`, top-level names shown in the request ≤ 10 entries × 128 chars, control characters stripped. | Accepted 2026-11-09 |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F04-01 | A message round-trips through framing unchanged | §18.4 | T | `protocol::tests::round_trip_preserves_message` | Passing |
| AC-F04-02 | A 4 GiB length prefix is rejected before any allocation | §18.4, §93 | T | `oversized_length_is_rejected_before_reading_body` | Passing |
| AC-F04-03 | A truncated body is a clean error, not a hang | §90 | T | `truncated_body_errors_cleanly` | Passing |
| AC-F04-04 | Different majors are refused; minors negotiate down | FR-SESS-001, §58 | T | `negotiation_refuses_different_majors`, `negotiation_takes_the_lower_minor` | Passing |
| AC-F04-05 | Zero chunk size is refused at negotiation | §92, §93 | T | `a_zero_chunk_size_is_refused_at_negotiation` | Passing (uncommitted) |
| AC-F04-06 | A chunk size below 64 KiB or above 16 MiB is refused | D-F04-3 | T | `protocol::tests::chunk_size_outside_bounds_is_refused` | Passing (uncommitted) |
| AC-F04-07 | An unknown message `type` is rejected, not ignored | §18.4 | T | `protocol::tests::unknown_message_type_is_rejected` | Passing (uncommitted) |
| AC-F04-08 | On version mismatch, both sides receive a `versionMismatch` error naming both versions | FR-SESS-003 | T | `session::tests::version_mismatch_is_reported_to_both_sides` | Passing (uncommitted) |
| AC-F04-09 | Given a `ChunkHeader` with a wrong size, an out-of-range index, an unexpected chunk or a foreign transfer id, then the receiver aborts with `protocolError` and writes nothing for it | §92 | T | `session::tests::hostile_chunk_headers_are_refused` | Passing (uncommitted) |
| AC-F04-10 | Given a receiver that rejects, then no `ChunkHeader` is ever sent | §48, NFR-SEC-006 | T | `session::tests::rejected_request_writes_nothing` | Passing (uncommitted) |
| AC-F04-11 | Given the user does not answer within 120 s, then the request is rejected | D-F04-5 | T | `session::tests::unanswered_request_times_out_as_reject` | Passing (uncommitted) |
| AC-F04-12 | Given a peer that stalls mid-handshake, then the session ends within 10 s | §90 | T | `session::tests::stalled_handshake_times_out` | Passing (uncommitted) |
| AC-F04-13 | No file data is sent before authentication | FR-SESS-004 | T + A | planned (F08) | **Blocked on F08** |

## 6. Out of scope

- Cryptographic handshake → F08.
- Chunk scheduling, resume, verification → F06.
- QUIC → F09.

## 7. Current implementation

**Progress 2026-11-09 (uncommitted):** [`session.rs`](../src-tauri/src/session.rs) implements the D-F04-2 sequence with the D-F04-5 timeouts, chunk-size bounds (64 KiB–16 MiB), version-mismatch notification, and full validation of every remote field. AC-F04-01…12 pass. AC-F04-13 waits for F08; meanwhile release builds refuse to transfer (D-F08-7, AC-F08-13).

- [`protocol.rs`](../src-tauri/src/protocol.rs): `Message` enum (all §19
  messages), framing with the 16 MiB cap, `ProtocolVersion::negotiate`,
  `Capabilities::intersect` (rejects zero chunk size, uncommitted).
- Missing: the session state machine (sender and receiver), timeouts, the
  incoming-request hand-off to the UI, chunk-header validation, mismatch
  notification, chunk-size bounds.
