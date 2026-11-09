# F06 — Chunking, resume and integrity

| | |
|---|---|
| **Status** | Approved 2026-11-09 — In Progress — restart resume (D-F06-5) not built |
| **SRS phase** | 6 (§108) |
| **SRS sources** | §21 (FR-CHUNK-001…004), §22 (FR-RESUME-001…006), §23, §52, §53, §55, §94, §103, §105, FR-LINK-009, FR-TRANS-007, R-05, R-10 |
| **Depends on** | F04, F07 |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

A transfer interrupted by a pulled cable, a sleeping laptop or a closed app
resumes where it stopped, sends only what is missing, and is never presented as
complete unless every byte matched its SHA-256.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| FR-CHUNK-001 | A chunk is the atomic unit of resume; partial chunks never count | shall | VALID | Partial: protocol (`ChunkHeader`/`ChunkFooter`) and manifest validation; no engine |
| FR-CHUNK-002 | Chunk size configurable and optimised per transport | shall | **VALID WITH CHANGES** — finding 1 | Partial: one default (4 MiB) |
| FR-CHUNK-003 | Chunk size negotiated at session start | shall | VALID WITH CHANGES — needs bounds (F04 D-F04-3) | Implemented (`intersect`, zero refused, uncommitted) |
| FR-CHUNK-004 | Chunk metadata without excessive filesystem overhead | shall | **VALID WITH CHANGES** — finding 2 | Not started |
| FR-RESUME-001 | Interrupted transfers are resumable | shall | VALID | Partial |
| FR-RESUME-002 | Receiver reports completed chunks | shall | VALID | Partial: `Accept{completed_chunks}`, `ResumeManifest` |
| FR-RESUME-003 | Sender sends only missing or invalid chunks | shall | VALID | Partial: `ResumeManifest::validate`, `missing_chunks` |
| FR-RESUME-004 | Resume metadata survives restart where practical | shall | VALID WITH CHANGES — finding 3 | Not started |
| FR-RESUME-005 | A partial transfer is never presented as complete | shall | VALID | Partial: `.ftpart` files and `finalize_file` (uncommitted) |
| FR-RESUME-006 | Auto-resume on replug per settings | may | VALID | Not started |
| §23 | SHA-256 after transfer, streamed; failure means failed | shall | VALID | Not started (`sha2` is a dependency) |

## 3. Validity findings

1. **"Optimised for the active transport" (FR-CHUNK-002) cannot be tested.** No
   target or rule is given. → CR-12. Until F09 measures it, one default (4 MiB)
   with negotiation is the honest implementation.
2. **"Excessive filesystem overhead" (FR-CHUNK-004) has no threshold.** → CR-12.
   Proposed: resume state costs at most one bit per chunk plus O(number of files),
   in one sidecar file per transfer, never one file per chunk.
3. **Restart recovery is "where practical" here but unconditional elsewhere.**
   §106 Reliability: "application restart recovers resumable transfers"; §53:
   "restore … where safe". → **CR-14**: pick one bar.
4. **SHA-256 proves integrity, not authenticity.** Until F08, the sender supplies
   both the data and its digests, so a hostile sender passes verification with
   any content. Verification catches corruption and resume holes; it is not a
   security control on its own. Recorded so nobody relies on it as one.
5. **`ResumeManifest` overstates what it prevents.** Its doc comment says an
   unvalidated manifest lets a peer "claim everything is done so … the transfer
   'completes' empty". Validation rejects out-of-range indices; it cannot stop a
   receiver from lying about chunks it holds — and a lying receiver only harms
   itself, because the end-to-end check (D-F06-3) runs on its own disk. Fix the
   comment.
6. **Disk full is a pause, not a failure, in §105** ("Transfer paused"). The
   error is already resumable (`Error::DiskFull`); the status mapping must follow.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F06-1 | When does a chunk count? | Only after the receiver read exactly the expected length, the footer digest matched, and the write to the part file returned. Not before. | Accepted 2026-11-09 |
| D-F06-2 | Where do incomplete files live? | `<name>.<first 8 of transfer id>.ftpart` beside the destination; renamed only after D-F06-3 passes (implemented: `filesystem::part_path`, `finalize_file`). | Accepted in code (uncommitted) |
| D-F06-3 | End-to-end check | Sender hashes every chunk as it reads it, including chunks skipped on resume, and sends per-file digests in `TransferComplete`. Receiver re-reads each part file from disk, compares, then finalises. Mismatch → `VerificationFailed`, part deleted, `FAILED`. | Accepted 2026-11-09 |
| D-F06-4 | In-process resume | Receiver keeps each session keyed by transfer id, sender device id and a manifest fingerprint (paths, sizes, chunk size). A reconnect that matches all three gets `Accept{completed_chunks}`; anything else is a new request needing approval. | Accepted 2026-11-09 |
| D-F06-5 | Resume across restart | Receiver writes `<destination root>/.fasttransfer/<transfer id>.json` (completed-chunk bitmap, fingerprint, chunk size) by write-temp-then-rename, after `sync_data` on the part file, at most every 2 s and on every pause or interrupt. Sender keeps its job (source paths, id) in the app data folder. MVP may ship in-process resume only if CR-14 allows. | Accepted 2026-11-09 |
| D-F06-6 | Re-approval | A resume after restart is shown to the receiver again until F08 can prove the sender's identity — before F08 a transfer id is the only link, and it travels in clear text. | Accepted 2026-11-09 |
| D-F06-7 | Status mapping | Link drop or idle timeout → `INTERRUPTED`; user pause → `PAUSED`; disk full → `PAUSED` with the disk-full error; digest mismatch or protocol violation → `FAILED`; user cancel → `CANCELLED`. | Accepted 2026-11-09 |
| D-F06-8 | Retry after a verification failure | No automatic retry. "Retry" (§27) starts the file again from zero. | Accepted 2026-11-09 |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F06-01 | The chunk plan covers the file with no gap or overlap, and its length equals `chunk_count` | FR-CHUNK-001 | T | `transfer::tests::chunk_plan_covers_the_whole_file`, `chunk_plan_chunks_are_contiguous`, `chunk_count_matches_plan_length` | Passing |
| AC-F06-02 | A zero chunk size is an error, not a panic | §93 | T | `chunk_plan_rejects_a_zero_chunk_size_instead_of_panicking` | Passing (uncommitted) |
| AC-F06-03 | A resume manifest naming a chunk or file that cannot exist is refused | FR-RESUME-002, §92 | T | `validate_rejects_a_chunk_index_beyond_the_file`, `validate_rejects_a_file_index_beyond_the_manifest` | Passing |
| AC-F06-04 | Given the connection drops in the middle of a chunk, then that chunk is not counted, and the resume sends it again | FR-CHUNK-001, R-05 | T | `session::tests::partial_chunk_is_never_counted` | Passing (uncommitted) |
| AC-F06-05 | Given an interruption after N of M chunks, when resumed, then exactly M − N chunks cross the wire and the result verifies | FR-RESUME-001, -003 | T | `session::tests::pause_then_resume_completes`, `partial_chunk_is_never_counted` | Passing (uncommitted) |
| AC-F06-06 | Given one flipped bit in a chunk in transit, then the footer check fails, nothing is finalised, and the transfer is `FAILED` | §23 | T | `session::tests::tampered_chunk_is_detected` | Passing (uncommitted) |
| AC-F06-07 | Given a sender whose `TransferComplete` digest is wrong, then the receiver reports `VerificationFailed` and no final file exists | §23, FR-RESUME-005 | T | `session::tests::end_to_end_mismatch_fails_the_transfer` | Passing (uncommitted) |
| AC-F06-08 | At no point before verification does a file exist under its final name | FR-RESUME-005 | T | `session::tests::pause_then_resume_completes` (only a `.ftpart` exists while paused) | Passing (uncommitted) |
| AC-F06-09 | A finalised file never overwrites under the default policy | FR-RESUME-005, §24 | T | `filesystem::tests::finalising_with_keep_both_never_overwrites` | Passing (uncommitted) |
| AC-F06-10 | Given the receiver restarts mid-transfer, when the sender reconnects, then only missing chunks are sent | FR-RESUME-004 | T | planned: `session::tests::resume_survives_receiver_restart` | Planned (needs D-F06-5) |
| AC-F06-11 | Given the disk fills, then the transfer is `PAUSED`, data so far is kept, and the error is `diskFull` with a remedy | §105, §95 | T | planned: `session::tests::disk_full_pauses`; `error::tests::disk_full_error_reports_resumable` | Implemented (orchestrator maps `diskFull` to `PAUSED`); full-disk path not yet tested |
| AC-F06-12 | Unplug the Ethernet cable mid-transfer, replug, resume: the file verifies | FR-LINK-009, §103 | D | Manual | Planned — **feasible on the reference hardware** |
| AC-F06-13 | The same over Thunderbolt | §88 | D | Manual | **CONDITIONAL (§5.2)** |

## 6. Out of scope

- Throughput tuning of chunk size → F09.
- Who may resume a transfer → F08 identity, D-F06-6 meanwhile.

## 7. Current implementation

**Progress 2026-11-09 (uncommitted):** per-chunk SHA-256 checked before a chunk is queued; a chunk counts only after its write returns; part files renamed only after every file is re-hashed from disk; in-process resume keyed by transfer id, peer id and manifest fingerprint; status mapping D-F06-7 in the orchestrator. Passing: AC-F06-01…09. Not built: persistence across restart (D-F06-5, AC-F06-10).

- [`transfer.rs`](../src-tauri/src/transfer.rs): `chunk_plan`, `chunk_count`,
  `ResumeManifest::{validate, missing_chunks}`, `PIPELINE_DEPTH`, `pipeline()`.
- [`filesystem.rs`](../src-tauri/src/filesystem.rs): `part_path`,
  `finalize_file`, `map_write_error` (uncommitted).
- Missing: the chunk loop, per-chunk and end-to-end hashing, the session table,
  persistence, status mapping.
