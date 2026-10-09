# F08 — Pairing, authentication and encryption

| | |
|---|---|
| **Status** | Draft — **needs a security review before approval** (R-03) |
| **SRS phase** | 7 (§108) |
| **SRS sources** | §11.5, §14, §15 (FR-PAIR-001…009), §34 (NFR-SEC-001…009), §35, §36, §81, §90, R-03, D-03, D-07 |
| **Depends on** | F04 |
| **Verdict** | NEEDS CHANGE (blocking) |
| **Last validated** | 2026-11-09 |

## 1. Intent

Before any byte of a file moves, both computers know they are talking to the
machine the user approved, and nobody else on the link can read or alter the
transfer. §5.1 makes this **guaranteed**, not optional.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| FR-PAIR-001 | User explicitly approves a new device | must | VALID | Partial: dialog exists, not bound to any session |
| FR-PAIR-002 | Show name, OS, id, connection type, link speed | shall | VALID | Partial |
| FR-PAIR-003 | Short code shown on both ends, must match | shall | **NEEDS CHANGE** — findings 1–2 | **Incorrect:** each side makes its own random code |
| FR-PAIR-004 | Codes single-use and time-limited (120 s) | shall | VALID | Implemented (`PairingState`) |
| FR-PAIR-005 | Rate-limit pairing attempts | shall | VALID | **Defect:** see finding 4 |
| FR-PAIR-006 | QR pairing in a future release | should | VALID | Deferred to v1.1 (§100) |
| FR-PAIR-007 | Trusted devices may be remembered | may | VALID | Not started: the trusted list is never filled or saved |
| FR-PAIR-008 | Revoke a trusted device | shall | VALID | Partial: in memory only |
| FR-PAIR-009 | Secrets in OS-protected storage | must | VALID | Not started (no key exists) |
| NFR-SEC-001 | All authenticated transfers encrypted | shall | **NEEDS CHANGE** — finding 3 | Not started |
| NFR-SEC-002 | Discovery grants no filesystem access | shall | VALID | Holds (F03, F05 D-F05-1) |
| NFR-SEC-003 | Users explicitly authorise new devices | shall | VALID (same as FR-PAIR-001) | Partial |
| NFR-SEC-004 | Established, maintained crypto libraries | shall | VALID | Holds by inspection, but see finding 5 |
| NFR-SEC-005 | Private keys in OS secure storage | shall | VALID | Not started |
| NFR-SEC-006 | No file data before authorisation | shall | VALID | Not started (F04 AC-F04-13) |
| NFR-SEC-007 | Logs never contain file contents | shall | VALID | Holds by inspection |
| NFR-SEC-008 | No credentials or keys in logs | shall | VALID | Holds (no keys exist yet) |
| NFR-SEC-009 | Documented handshake from an established library; Noise XX recommended | shall | **VALID WITH CHANGES** — finding 2 | Not started; `Hello`/`HelloAck` nonces are a custom shape (F04 finding 2) |

## 3. Validity findings

1. **FR-PAIR-003 does not say where the code comes from, and the code shows why
   that matters.** `pair_device` calls `generate_pairing_code()`, an independent
   random number on each machine. Two independent random codes never match, and
   `confirm_pairing` compares what the user typed with the code shown on the same
   screen. The flow authenticates nothing. A comparison code is only meaningful if
   both sides derive it from the same cryptographic exchange. → **CR-03**.
2. **A short code compared after a Noise XX handshake is not enough on its own.**
   If the six digits are just derived from the handshake hash, a man in the
   middle runs one handshake with each side and keeps trying ephemeral keys until
   both six-digit codes agree — about a million attempts, which takes seconds.
   Protocols that use comparison codes (ZRTP, Bluetooth numeric comparison) add a
   commitment step for exactly this reason. Building that commitment by hand is
   the custom handshake construction NFR-SEC-009 prohibits. The standard tool for
   "short code + strong key" is a PAKE (for example SPAKE2): one screen shows the
   code, the other user types it, and an attacker gets one online guess per
   attempt — which the rate limit then caps. The existing `PairingDialog` already
   has the user type the code. → **CR-03**, D-F08-2. **Needs the R-03 review.**
3. **NFR-SEC-001 has a loophole.** "All *authenticated* transfers shall be
   encrypted" leaves room for unauthenticated, unencrypted ones. §5.1 says
   "Encrypt all transfers". Separately, §98's MVP list includes "Device pairing"
   but not encryption, so the MVP is non-conformant to §5.1 by construction. →
   **CR-04**.
4. **The pairing rate limit resets after 120 s, not 300 s.**
   `PAIRING_RATE_WINDOW` (300 s) is defined in `security.rs` and never used.
   Attempts live on the pending pairing, which expires with its code after 120 s;
   the next `begin` creates a fresh entry with zero attempts.
   `docs/api-reference.md` says "three wrong attempts rate-limit the device for
   five minutes", which is false. → **CD-01**.
5. **`constant_time_eq` is hand-written.** It is a comparison, not a primitive,
   but it is exactly the kind of code an inspection under NFR-SEC-004 flags, and
   the optimiser is not obliged to keep it constant-time. Use `subtle`. →
   **CD-05**. With a PAKE (finding 2) the code comparison disappears anyway.
6. **§36 "subject to the security model" names no model.** → D-F08-3.
7. **Every transfer today is plaintext and unauthenticated.** Discovery
   (F03) and transfer (F05) can be built and tested first, but a build that sends
   files before this feature lands breaks §5.1, FR-SESS-004, NFR-SEC-001 and
   NFR-SEC-006. → D-F08-7 needs an explicit owner decision.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F08-1 | Session protocol | Noise `XX_25519_ChaChaPoly_BLAKE2s` via the `snow` crate (new dependency). Each device has a long-term X25519 static key. After pairing, a peer's static public key is pinned; later sessions must present the pinned key or fail. | Proposed — needs review |
| D-F08-2 | Pairing | SPAKE2 (RustCrypto `spake2` crate, new dependency): device A displays a 6-digit code, the user types it on B. The SPAKE2 key authenticates an exchange of the two Noise static public keys, which are then pinned. Codes stay single-use, 120 s, 3 failures per 300 s. FR-PAIR-003 is reworded to "shown on one endpoint and entered on the other" (CR-03). | Proposed — needs review |
| D-F08-3 | Trusted reconnects (§36) | A pinned key skips pairing, never the incoming-transfer prompt — unless the receiver turns on "Accept from this device without asking" for that device. | Proposed |
| D-F08-4 | Key storage | `keyring` crate (Credential Manager, Keychain, Secret Service). Where unavailable: a file readable only by the user, plus a recorded security review (D-07). | Proposed — new dependency |
| D-F08-5 | Rate limit | Count failures per peer device id *and* per source address over a 300 s sliding window that outlives the code; cap pending pairings at 8. | Proposed |
| D-F08-6 | Constant-time compare | `subtle::ConstantTimeEq` wherever a secret is compared. | Proposed |
| D-F08-7 | Until F08 lands | Plaintext transfers allowed **only** in development builds, with a visible "Unencrypted preview" label and per-transfer receiver approval. Release builds refuse to transfer without F08. The owner must accept this or block F05 until F08. | **Owner decision required** |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F08-01 | A code is six digits, varies between calls, and comes from the OS CSPRNG | FR-PAIR-003 | T + I | `security::tests::pairing_code_has_the_displayed_shape`, `pairing_codes_are_not_constant` | Passing |
| AC-F08-02 | A code is single-use and expires after 120 s | FR-PAIR-004 | T | `a_consumed_code_cannot_be_reused` | Passing (expiry untested) |
| AC-F08-03 | Three wrong codes block the device for 300 s, **even after the code expires** | FR-PAIR-005 | T | planned: `security::tests::rate_limit_outlives_code_expiry` | **Expected to fail today** (CD-01) |
| AC-F08-04 | Given the right code, both devices end with each other's static key pinned; given a wrong code, neither does | FR-PAIR-003 | T | planned: `security::tests::pake_pairs_only_with_the_right_code` | Planned |
| AC-F08-05 | Given a relay in the middle of the pairing, then pairing fails unless the relay guesses the code, and each failed guess counts towards the limit | FR-PAIR-003, R-03 | T + A | planned: `security::tests::relayed_pairing_fails` | Planned |
| AC-F08-06 | Given a loopback transfer of a file containing a marker string, then the marker never appears in the captured bytes on the wire | NFR-SEC-001, §106 | T | planned: `session::tests::wire_bytes_are_encrypted` | Planned |
| AC-F08-07 | Given an unpaired device, then it cannot reach `TransferRequest` | FR-SESS-004, NFR-SEC-006 | T | planned: `session::tests::unpaired_peer_cannot_request` | Planned |
| AC-F08-08 | Given a revoked device, then its next session fails and pairing is required again | FR-PAIR-008 | T | planned: `security::tests::revoked_key_is_refused` | Planned |
| AC-F08-09 | Trusted devices survive an application restart | FR-PAIR-007, §53 | T | planned: `security::tests::trust_store_round_trips` | Planned |
| AC-F08-10 | The private key is never in `identity.json`, logs or IPC payloads | NFR-SEC-005, -008 | T + I | planned: `security::tests::private_key_never_serialised` | Planned |
| AC-F08-11 | The dependency tree contains only established crypto crates; the repo contains no hand-written primitive or handshake step | NFR-SEC-004, -009 | I | Review checklist | Planned |
| AC-F08-12 | A recorded security review of D-F08-1/2 exists before Phase 7 ships | R-03, §114.4 | A | Review record | Planned |

## 6. Out of scope

- QR pairing (FR-PAIR-006) → v1.1.
- Encrypting data at rest on the receiver.

## 7. Current implementation

- [`security.rs`](../src-tauri/src/security.rs): `PairingState` (code TTL,
  single use, attempt counter), `generate_pairing_code`, `constant_time_eq`,
  `IdentityStore` (device id and display name only; `public_key` is empty).
- [`commands.rs`](../src-tauri/src/commands.rs): `pair_device`,
  `confirm_pairing` (never records a trusted device), trust list in memory.
- [`protocol.rs`](../src-tauri/src/protocol.rs): `Authenticate` carries an empty
  `proof`; `Capabilities.encryption` is always `false`.
- Missing: everything cryptographic. New dependencies needed: `snow`, `spake2`,
  `keyring`, `subtle` — each needs approval.
