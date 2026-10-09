# API Reference — Tauri IPC

The contract between the React frontend and the Rust core. Mirrors SRS §79.

**Every change here must be paired.** A Rust struct in `src-tauri/src/` and its
mirror in `src/types/index.ts` that drift apart compile cleanly and fail at
runtime, usually in front of a user.

## Commands

Arguments and results are camelCase on the JS side (`deviceId`), snake_case in
Rust (`device_id`) — Tauri converts automatically. The tables below use the JS
spelling.

### Link and transport

| Command | Arguments | Returns |
|---|---|---|
| `get_link_info` | — | `LinkInfo` |
| `get_transports` | — | `TransportCandidate[]` |
| `get_devices` | — | `Peer[]` |

`get_link_info` is the one to call before showing any speed figure. It returns
both the classification and the user-facing explanation, so the UI never has to
invent a reason a cable does not work.

### Diagnostics

| Command | Arguments | Returns |
|---|---|---|
| `get_diagnostics` | — | `Diagnostics` |

`Diagnostics.notes` carries plain-language explanations of likely problems —
including the Windows "direct cable link classified as Public" case, which
silently blocks discovery and is otherwise very hard for a user to diagnose.

### Pairing and trust

| Command | Arguments | Returns |
|---|---|---|
| `pair_device` | `deviceId`, `displayName`, `platform` | `PairingChallenge` |
| `confirm_pairing` | `deviceId`, `code` | `void` |
| `get_trusted_devices` | — | `TrustedDevice[]` |
| `remove_trusted_device` | `deviceId` | `void` |

`pair_device` returns a code **to display**. The user compares it with the code
on the other computer; `confirm_pairing` submits what they report seeing.

Rules enforced in the core:

- the code is single-use — a confirmed pairing is removed immediately
- the code expires after 120 seconds
- three wrong attempts rate-limit the device for five minutes
- re-requesting a pairing does **not** reset the attempt counter
- comparison is constant-time

### Transfers

| Command | Arguments | Returns |
|---|---|---|
| `start_transfer` | `deviceId`, `paths` | `transferId` |
| `get_transfer` | `transferId` | `TransferProgress` |
| `list_transfers` | — | `TransferProgress[]` |
| `pause_transfer` | `transferId` | `void` |
| `resume_transfer` | `transferId` | `void` |
| `cancel_transfer` | `transferId` | `void` |

`paths` are **local paths the user picked through the OS dialog**. They are
trusted. Paths arriving from a peer never flow through this argument — those go
through `filesystem::safe_join` inside the core (SRS §91).

`resume_transfer` rejects a transfer that is not in a resumable state rather
than silently accepting it.

### Settings

| Command | Arguments | Returns |
|---|---|---|
| `get_settings` | — | `ApplicationSettings` |
| `update_settings` | `settings` | `void` |

## Events

Subscribe with `api.on<T>(name, handler)`. It returns the unlisten function —
pass it to the effect cleanup, or listeners accumulate across re-renders.

| Event | Payload | When |
|---|---|---|
| `device_discovered` | `Peer` | a peer appears |
| `device_updated` | `Peer` | a peer's details change |
| `device_removed` | `{ deviceId }` | a peer goes away or expires |
| `link_changed` | `LinkInfo` | the cable is plugged, unplugged, or reclassified |
| `transfer_started` | `TransferProgress` | a transfer is queued |
| `transfer_progress` | `TransferProgress` | periodic progress |
| `transfer_paused` | `TransferProgress` | paused by the user |
| `transfer_resumed` | `TransferProgress` | resumed |
| `transfer_completed` | `TransferProgress` | finished **and verified** |
| `transfer_failed` | `TransferProgress` | failed; carries the error |
| `transfer_cancelled` | `TransferProgress` | cancelled |
| `connection_changed` | `LinkInfo` | transport changed mid-session |
| `diagnostic_update` | `Diagnostics` | diagnostics refreshed |

`transfer_completed` is emitted only after verification has passed. A transfer
whose SHA-256 check failed emits `transfer_failed` — it is never reported as
complete (SRS FR-RESUME-005).

## Errors

Every command rejects with a structured `AppError`:

```ts
interface AppError {
  kind: AppErrorKind;      // stable tag to branch on
  what: string;            // what happened
  why: string | null;      // why, when known
  remedy: string | null;   // what the user can do
  resumable: boolean;      // whether a Resume affordance is valid
}
```

`src/services/api.ts` normalizes anything non-conforming — including a panic,
which arrives as a plain string — into this shape before it reaches a component.

### Error kinds

| Kind | Meaning | Resumable |
|---|---|---|
| `linkUnusable` | cable present, no transfer path | no |
| `noTransport` | nothing usable at all | no |
| `peerNotFound` | other computer not found | yes |
| `connectionLost` | cable out, or peer gone | yes |
| `peerRejected` | the other user declined | no |
| `authenticationFailed` | pairing failed or expired | no |
| `versionMismatch` | incompatible builds | no |
| `protocolError` | malformed or oversized message | no |
| `unsafePath` | **path traversal blocked** | no |
| `destinationNotWritable` | permission refused | no |
| `diskFull` | no space at the destination | yes |
| `verificationFailed` | SHA-256 mismatch | yes |
| `transferNotFound` | unknown id | no |
| `cancelled` | user cancelled | no |
| `io` | file operation failed | yes |
| `internal` | unexpected | no |

`unsafePath` means the app **refused to write** and nothing was created. It is a
security event, not an I/O hiccup — if a user reports seeing it repeatedly,
investigate rather than retry.

## Statuses

`TransferStatus`: `QUEUED`, `PREPARING`, `CONNECTING`, `AUTHENTICATING`,
`TRANSFERRING`, `VERIFYING`, `COMPLETED`, `PAUSED`, `INTERRUPTED`, `FAILED`,
`CANCELLED`.

Terminal: `COMPLETED`, `FAILED`, `CANCELLED`.
Resumable: `INTERRUPTED`, `PAUSED`, `FAILED`.

`COMPLETED` is reachable only through `VERIFYING` having passed.
