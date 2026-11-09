# F10 — Desktop UI

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 9 (§108) |
| **SRS sources** | §38–§48, §54, §55, §66 (NFR-A11Y-001), §67, §68, §79, §80, §81, FR-TRANSFER-003, FR-TRANS-008, §101 |
| **Depends on** | F01, F03, F05, F06 |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

Connect → Discover → Select → Send → Verify → Done (§111), with no networking
knowledge needed, and every number on screen either a measurement or clearly a
rating.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| §38 | Main screen: link, measured speed, nearby devices, drop zone, Select Files | — (no ID) | VALID | Partial: banner, device list, static drop zone |
| §39 | Device card: name, OS, connection, quality, trust, availability | — | VALID | Partial (`DeviceCard`) |
| §40, §41 | Transfer screen and completion summary | — | VALID | Not started |
| §42, §95 | Errors say what, why and what to do | shall | VALID | Implemented (`ErrorNotice`, `AppError`) |
| §44, §45 | Settings; default download folder | — | VALID | Not started |
| §46 | History, metadata only | shall | VALID | Partial: `services/database.ts` (localStorage) |
| §47 | Desktop notifications, user can disable | — | VALID | Not started (plugin registered) |
| §48 | Incoming transfer: review, Accept, Reject | must | VALID | Not started |
| §54, §55 | Sleep/wake and connection-loss screens | — | VALID | Not started |
| NFR-A11Y-001 | WCAG 2.1 AA | shall | VALID | Unverified; dialogs use ARIA roles |
| §67 | All user-facing strings externalised | shall | **VALID WITH CHANGES** — finding 2 | Not started |
| §68 | Light / dark / system theme | — | VALID | Not started (Tailwind `darkMode: "class"`, no switch) |
| §79 | IPC commands and events | shall | **VALID WITH CHANGES** — finding 1 | Partial; drifted from the SRS |
| §80 | Frontend never duplicates authoritative state | shall | VALID | **Gap:** settings and history are kept in `localStorage` |
| §81 | Minimal Tauri capabilities | shall | VALID | Partial: `fs:allow-read-text-file` granted though no UI code reads files |
| FR-TRANS-008 | Always show the transport in use | shall | VALID | Not started |

## 3. Validity findings

1. **§79 and the code have drifted apart.** In the SRS but not the code:
   `select_files`, `retry_transfer`, `get_transfer_history`. In the code but not
   the SRS: `confirm_pairing`, `get_trusted_devices`, `list_transfers`,
   `error_shape`. In neither, though §48 requires them: an `incoming_transfer`
   event and `accept_transfer` / `reject_transfer` commands. → **CR-06**.
2. **§67 versus the error design.** "All user-facing strings shall be
   externalised", but `error.rs` builds English sentences in Rust, as CLAUDE.md
   instructs ("map to user-facing strings in `error.rs`"). → **CR-16**: Rust sends
   a stable `kind` plus parameters; the UI owns the words.
3. **The UI listens for events nothing sends.** `App.tsx` subscribes to
   `link_changed`, `device_discovered`, `device_updated` and `device_removed`;
   the core emits only `transfer_started/cancelled/paused/resumed`. → CD-04, fixed
   by F01 D-F01-3 and F03 D-F03-5.
4. **Destination ambiguity in §102.** See F05 finding 1 (CR-15).

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F10-1 | MVP screens | Main (banner, devices, drop zone), Incoming request dialog, Transfers panel (progress, measured speed, pause/resume/cancel), Completion summary with "Verified", Settings (destination, conflict default, notifications, theme), Diagnostics. History is v1.0, not MVP (§98). | Proposed |
| D-F10-2 | Strings | `src/i18n/en.ts` keyed by UI id and error `kind`; components never hard-code sentences. | Proposed |
| D-F10-3 | State | Only Rust events and command results change transfer, peer and link state; settings and history move to the core (persisted in the app data folder) and the UI caches them. | Proposed |
| D-F10-4 | Accessibility checks | Automated: `vitest-axe` on each screen (new dev dependency). Manual: keyboard-only walkthrough and a screen-reader pass per release. | Proposed — new dependency |
| D-F10-5 | Speed display | Two labelled values always: "Link: 40 Gbps (rated)" and "Current: 2.3 GB/s (measured)". Never one without its label (§101). | Proposed |
| D-F10-6 | Capabilities | Remove `fs:allow-read-text-file` and the `fs` plugin unless a UI feature needs it. | Proposed |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F10-01 | Every error rendered shows what happened, and why and what to do when known | §42, §95 | T | planned: `ErrorNotice.test.tsx` | Planned |
| AC-F10-02 | Given an `incoming_transfer` event, the dialog shows sender name and OS, file count, up to 10 top-level names, total size, destination, and Accept/Reject | §48 | T | planned: `IncomingTransferDialog.test.tsx` | Planned |
| AC-F10-03 | A transfer row shows the transport, the measured speed and the link rating as separate labelled values | §101, FR-TRANS-008 | T | planned: `TransferRow.test.tsx` | Planned |
| AC-F10-04 | Components never call `invoke` directly | CLAUDE.md, §81 | I + lint | planned: ESLint `no-restricted-imports` for `@tauri-apps/api/core` outside `services/` | Planned |
| AC-F10-05 | Peer, link and transfer state change only in response to core events or command results | §80 | T | planned: `App.test.tsx` | Planned |
| AC-F10-06 | Every interactive control is reachable by keyboard with a visible focus ring; axe reports no AA violations | NFR-A11Y-001 | T + D | planned: `a11y.test.tsx` | Planned |
| AC-F10-07 | No user-facing sentence is hard-coded in a component | §67 | I | planned: lint rule or review | Planned |
| AC-F10-08 | Theme follows the system by default and can be forced light or dark | §68 | T | planned: `theme.test.ts` | Planned |
| AC-F10-09 | Notifications fire for the §47 events and stop when disabled | §47 | T | planned: `notifications.test.ts` | Planned |
| AC-F10-10 | In a development build without F08, every screen that can send or receive shows a persistent "Unencrypted preview" label that cannot be dismissed | D-F08-7 | T | planned: `PreviewLabel.test.tsx` | Implemented (`PreviewLabel`), not yet tested |

## 6. Out of scope

- Transfer logic of any kind (§11.1).

## 7. Current implementation

- [`App.tsx`](../src/App.tsx), [`LinkBanner`](../src/components/dashboard/LinkBanner.tsx),
  [`DeviceCard`](../src/components/dashboard/DeviceCard.tsx),
  [`PairingDialog`](../src/components/auth/PairingDialog.tsx),
  [`ErrorNotice`](../src/components/shared/ErrorNotice.tsx);
  [`services/api.ts`](../src/services/api.ts), `auth.ts`, `database.ts`.
- The TS mirror in [`types/index.ts`](../src/types/index.ts) does not yet have
  `TransferProgress.direction`, `peerName` or `error` (added on the Rust side,
  uncommitted) — a mirror gap CLAUDE.md forbids. Must be closed with F05.
