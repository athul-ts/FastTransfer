# F11 — Diagnostics, errors and logging

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 9 (§108) |
| **SRS sources** | §5.3, §37 (FR-FW-001), §42, §43, §51, §52, §95, NFR-SEC-007, NFR-SEC-008, R-04 |
| **Depends on** | F01, F02, F03 |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

When something does not work — most often the firewall or a cable that cannot
carry data — the user can see why without reading a log, and the logs that do
exist never leak file contents or keys.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| §37 | Detect likely firewall problems, explain them; never disable the firewall | shall / must not | VALID | Partial: a generic note in `get_diagnostics` |
| FR-FW-001 | Windows: detect a direct link classified "Public" and explain it | shall | VALID | Not started |
| §43 | Diagnostics: interfaces, cable, firewall, discovery, pairing | — (no ID) | VALID | Partial: `get_diagnostics` returns most fields; no UI |
| §51 | Structured levels, `INFO` default, never sensitive data; **rotation and a size cap** | shall | **VALID WITH CHANGES** — finding 1 | Partial: levels and default; no file, no rotation |
| §52 | A crash never marks an incomplete transfer as successful | must | VALID | Holds by design (F06 D-F06-2) |
| §95 | Every error states what, why (if known), what to do | shall | VALID | Implemented (`Error::classify`) |
| NFR-SEC-007 | No file contents in logs | shall | VALID | Holds by inspection |
| NFR-SEC-008 | No credentials or keys in logs | shall | VALID | Holds by inspection |

## 3. Validity findings

1. **§51 gives no rotation size or count.** → CR-12. Proposed: a log file in the
   app's log folder, 5 MiB per file, 3 files kept.
2. **§95's example needs free-space figures** ("Free space: 2.1 GB, Required:
   14.7 GB"). Today a full disk is noticed when a write fails, without a free-space
   number (`Error::DiskFull { available: None }`, uncommitted). Showing the number
   needs `GetDiskFreeSpaceExW` on Windows and `statvfs` on Unix. → D-F11-2.
3. **FR-FW-001 needs a Windows API that is not wired in.** The network profile
   ("Public"/"Private") comes from `INetworkListManager` (COM) or
   `Get-NetConnectionProfile`. → D-F11-3.
4. **The "every payload answers what" test only samples five variants.**
   `every_payload_answers_what_happened` should cover every `Error` variant, so a
   new variant cannot ship without a message.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F11-1 | Log destination | `tracing-appender` rolling file (new dependency) in the platform log folder plus stderr in debug builds; 5 MiB × 3. | Proposed — new dependency |
| D-F11-2 | Free space | Query free space at Accept time (`windows-sys` `Win32_Storage_FileSystem`, `libc::statvfs`) and refuse up front with `DiskFull { available: Some(..) }` when it is clearly insufficient; keep the write-failure path as the backstop. | Proposed |
| D-F11-3 | Public profile | Read the profile of the interface carrying the peer via `INetworkListManager`; when "Public", add the FR-FW-001 note with the exact Settings path. Never change the profile or the firewall. | Proposed |
| D-F11-4 | What may be logged | Device ids, transfer ids, sizes, chunk indices, timings, error kinds. Never file contents, never full local paths at `INFO` (file names only at `DEBUG`), never key material. | Proposed |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F11-01 | Every `Error` variant produces a non-empty `what`; unusable-link and disk-full errors carry a remedy | §95 | T | `error::tests::every_payload_answers_what_happened` (sample — extend to all variants) | Partly passing |
| AC-F11-02 | A disk-full error without a free-space figure does not invent one | §95 | T | `error::tests::disk_full_without_a_free_space_figure_does_not_invent_one` | Passing (uncommitted) |
| AC-F11-03 | Given a loopback transfer of a file containing a marker string, the captured logs never contain the marker | NFR-SEC-007 | T | planned: `session::tests::logs_never_contain_file_contents` | Planned |
| AC-F11-04 | Logs rotate at 5 MiB and keep 3 files | §51 | T | planned: `logging::tests::rotation_caps_disk_use` | Planned |
| AC-F11-05 | Diagnostics list interfaces with kind and virtual flag, link class and explanation, discovery method, peer count and trusted count | §43 | T | planned: `commands::tests::diagnostics_payload_is_complete` | Planned |
| AC-F11-06 | On Windows, a direct link on a "Public" profile produces the FR-FW-001 note | FR-FW-001 | D | Manual on the Windows reference machine | Planned — **feasible on the reference hardware** |
| AC-F11-07 | The app never changes firewall rules or network profiles | §37 | I | Review: no firewall/profile write APIs in the dependency tree or code | Planned |

## 6. Out of scope

- Error copy and localisation → F10 (CR-16).

## 7. Current implementation

- [`error.rs`](../src-tauri/src/error.rs): `Error`, `ErrorPayload`,
  `classify`, `Serialize`, `human_bytes`.
- [`commands.rs`](../src-tauri/src/commands.rs): `get_diagnostics` with notes;
  [`lib.rs`](../src-tauri/src/lib.rs): `init_tracing` (stdout, env filter).
