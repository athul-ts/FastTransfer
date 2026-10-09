# F05 — File and folder transfer

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 4 (files) and 5 (folders) (§108) |
| **SRS sources** | §7.2, §20 (FR-TRANSFER-001…010), §27, §45, §48, §57, §102, §106 "Transfer", §112 |
| **Depends on** | F02, F03, F04, F06, F07; F08 before release |
| **Verdict** | VALID |
| **Last validated** | 2026-11-09 |

## 1. Intent

Pick files or folders, choose a nearby computer, and they arrive intact in the
receiver's download folder, with progress, cancel and pause. This is the §112
milestone ("transfer 1 GB, verify SHA-256 on both ends") and the user-visible
core of the cable feature. It runs unchanged over Thunderbolt, Ethernet and Wi-Fi.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| FR-TRANSFER-001 | Select one or more files | shall | VALID | Not started (UI); manifest builder implemented, uncommitted |
| FR-TRANSFER-002 | Select folders | shall | VALID | Partial: `filesystem::build_manifest` walks folders (uncommitted) |
| FR-TRANSFER-003 | Drag and drop | shall | VALID | Not started (`dragDropEnabled: true`, no handler) |
| FR-TRANSFER-004 | Preserve relative directory structure | shall | VALID | Partial: manifest preserves it; nothing writes it yet |
| FR-TRANSFER-005 | Stream file contents | shall | VALID | Not started |
| FR-TRANSFER-006 | Large files, 100 GB+ tested | shall | VALID | Not started |
| FR-TRANSFER-007 | Display progress | shall | VALID | Partial: `TransferProgress`, `ThroughputEstimator` |
| FR-TRANSFER-008 | Cancel an active transfer | shall | VALID | Partial: state guard only (`can_cancel`, uncommitted) |
| FR-TRANSFER-009 | Pause and resume | should | VALID | Partial: state guards only |
| FR-TRANSFER-010 | Multiple queued transfers | shall | VALID | Not started |
| §45 | Default destination `Downloads/FastTransfer` | — (no ID) | VALID | Not started: `download_directory` defaults to empty |
| §48 | Receiver reviews and can reject | must | VALID | Not started |
| §57 | One active destination per transfer | — (no ID) | VALID | Holds by design |

## 3. Validity findings

1. **Who picks the destination folder?** §102's sender flow has "Select
   destination" and the receiver flow has "Choose destination"; §48 shows a fixed
   `Downloads/FastTransfer`. The sender cannot choose a folder on another
   computer. → **CR-15**: the sender chooses the *peer*; the receiver's folder is
   its setting (§45), optionally changed in the incoming dialog.
2. **Queue priority is inconsistent.** FR-TRANSFER-010 is "shall" and High
   (§96), but §98's MVP list omits a queue. → recorded; D-F05-5 keeps the queue
   minimal.
3. **§79 has no incoming-request IPC.** See F04 finding 3 (CR-06).
4. **§79's `select_files` command is unnecessary.** Files are picked in the
   webview with the Tauri dialog plugin, which returns paths; no Rust command is
   needed. → CR-06.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F05-1 | Authorisation before F08 exists | Every incoming transfer is shown to the receiver (sender name and OS, file count, top-level names, total size, destination) and needs Accept. This is the only gate until F08; dev builds only (see D-F08-7). | Proposed |
| D-F05-2 | Destination | Receiver setting, default `<Downloads>/FastTransfer` via `directories::UserDirs`, created on first use, refused if dangerous (F07). | Proposed |
| D-F05-3 | Sender flow | Device card → Send Files → OS picker (files or folder) → transfer starts. The current pairing-code step is removed from the send path until F08 redesigns it. | Proposed |
| D-F05-4 | Progress events | `transfer_progress` at most 4 per second, plus one on every status change. Payload adds `direction`, `peerName`, `error` (implemented in `TransferProgress`, uncommitted; TS mirror pending). | Proposed |
| D-F05-5 | Queue | One active transfer per peer; later ones wait as `QUEUED` and start in order. Reorder (§27) is v1.0, not MVP. | Proposed |
| D-F05-6 | Drag and drop | Listen for the Tauri drag-drop event; dropping on a device card sends to it; dropping on the drop zone sends to the selected device or asks which. | Proposed |
| D-F05-7 | Where the engine lives | New module `session.rs` (sender and receiver loops over any `AsyncRead + AsyncWrite`), so the loopback tests and production share one code path. Update CLAUDE.md layout and `docs/architecture.md`. | Proposed |

## 5. Acceptance criteria

All T criteria run over the in-memory or `127.0.0.1` loopback transport required
by CLAUDE.md.

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F05-01 | Given one file, when sent, then the receiver's file is byte-identical and both sides end `COMPLETED` | FR-TRANSFER-001 | T | planned: `session::tests::single_file_round_trip` | Planned |
| AC-F05-02 | Given a folder with nested folders, an empty folder and a zero-byte file, when sent, then the receiver's tree is identical | FR-TRANSFER-002, -004 | T | planned: `session::tests::folder_tree_round_trip` | Planned |
| AC-F05-03 | The manifest lists every folder before its contents, in a stable order | FR-TRANSFER-004 | T | `filesystem::tests::a_folder_keeps_its_structure_with_directories_before_contents` | Passing (uncommitted) |
| AC-F05-04 | Two picks with the same name, or names differing only in case, never collide | FR-TRANSFER-001 | T | `two_picks_with_the_same_name_do_not_collide`, `names_differing_only_by_case_do_not_collide_on_a_case_insensitive_receiver` | Passing (uncommitted) |
| AC-F05-05 | During a transfer, `transferredBytes` never decreases and ends equal to `totalBytes`; an empty transfer never shows `NaN` | FR-TRANSFER-007 | T | planned: `session::tests::progress_is_monotonic_and_complete`; `transfer::tests::fraction_handles_a_zero_byte_transfer` | Partly passing |
| AC-F05-06 | Given a cancel on either side mid-transfer, then both end `CANCELLED`, part files are removed, no final file appears | FR-TRANSFER-008 | T | planned: `session::tests::sender_cancel_cleans_up`, `receiver_cancel_stops_sender` | Planned |
| AC-F05-07 | Given a pause then resume, then the transfer completes and only unsent chunks cross the wire | FR-TRANSFER-009 | T | planned: `session::tests::pause_then_resume_completes` | Planned |
| AC-F05-08 | A completed or cancelled transfer cannot be paused or cancelled; no state is both pausable and resumable | FR-TRANSFER-008, -009 | T | `transfer::tests::only_a_moving_transfer_can_be_paused`, `a_finished_or_cancelled_transfer_cannot_be_cancelled`, `no_state_is_both_pausable_and_resumable` | Passing (uncommitted) |
| AC-F05-09 | Given the receiver rejects, then the sender ends with `peerRejected` and nothing is written | §48 | T | planned: `session::tests::rejected_request_writes_nothing` | Planned |
| AC-F05-10 | Given two transfers to one peer, then the second stays `QUEUED` until the first finishes | FR-TRANSFER-010 | T | planned: `commands::tests::second_transfer_waits_in_queue` | Planned |
| AC-F05-11 | Dropping files on a device card starts a transfer to that device | FR-TRANSFER-003 | T | planned: `DeviceCard.test.tsx` | Planned |
| AC-F05-12 | A 1 GiB file crosses the loopback with SHA-256 verified on both ends, and the measured throughput is reported | §112 | T (ignored by default; run on demand) | planned: `session::tests::one_gib_loopback_milestone` | Planned |
| AC-F05-13 | Windows ↔ Linux over a direct Ethernet cable, no router: 1 GB verified | §112, §73 | D | Manual | Planned — **feasible on the reference hardware** |
| AC-F05-14 | A 100 GB file transfers with bounded memory | FR-TRANSFER-006 | D | Manual (needs ~200 GB free across both machines, A-05) | Planned |
| AC-F05-15 | Same as AC-F05-13 over Thunderbolt | §112 | D | Manual | **CONDITIONAL (§5.2)** |

## 6. Out of scope

- Chunk mechanics, resume and verification → F06.
- Path safety, names, conflicts → F07.
- Authentication and encryption → F08 (release blocker).

## 7. Current implementation

- [`filesystem.rs`](../src-tauri/src/filesystem.rs) `build_manifest` and
  [`transfer.rs`](../src-tauri/src/transfer.rs) `prepare`, `check_totals`,
  `TransferProgress::new`, `TransferDirection`, state guards — mostly uncommitted.
- [`commands.rs`](../src-tauri/src/commands.rs) `start_transfer` records a
  `QUEUED` entry and discards its arguments.
- Missing: `session.rs`, the listener and accept loop, incoming-request IPC,
  receiver destination, queue, UI.
