# F07 — Filesystem safety and compatibility

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 4–5 (§108) |
| **SRS sources** | §24, §25 (FR-FS-001…003), §26 (FR-NAME-001…002), §49, §50, §91, §92, CLAUDE.md security rules 1–2 |
| **Depends on** | — |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

Nothing a peer sends can write outside the folder the receiver chose, overwrite a
file without consent, land in a system folder, or fail halfway because a name is
legal on one OS and not on the other. This is the critical security boundary
(§91).

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| §91 | Incoming paths never escape the destination | must | VALID | Implemented (`safe_join`, `verify_within`, `prepare`) |
| §92 | Validate filename, path, size, chunk index, hash, transfer id | must | VALID | Partial: paths, sizes, totals, duplicates, transfer id (uncommitted); chunk fields wait for F04 |
| §24 | Replace / Skip / Keep Both / Cancel, Keep Both default, destructive needs confirmation, "Apply to all" | shall | VALID | Partial: policy and `finalize_file`; no UI |
| §25 | Regular files and folders; should keep timestamps, permissions, symlinks where safe | shall / should | VALID WITH CHANGES — finding 3 | Partial: mtime applied; permissions and symlinks not transferred |
| FR-FS-001 | Unsupported metadata never fails the whole transfer | shall | VALID | Partial: metadata is best-effort |
| FR-FS-002 | Windows symlinks: sidecar or skip **with a clear warning** | shall | VALID | **Gap:** links are skipped with a debug log only |
| FR-FS-003 | Account for NTFS, APFS, ext4, btrfs, others | shall | **VALID WITH CHANGES** — finding 4 | Not started |
| §26 | Detect incompatible names, length limits, case, Unicode normalisation | shall | VALID | Partial: reserved names, characters, case collisions; not length or normalisation |
| FR-NAME-001 | NFC/NFD differences never duplicate or lose files | shall | VALID | Not started |
| FR-NAME-002 | Safe, non-destructive resolution | shall | VALID WITH CHANGES — finding 1 | Partial: automatic repair on the sender |
| §49 | Refuse dangerous destinations | shall | VALID | Implemented (`check_destination_root`, uncommitted) |
| §50 | Never silently elevate | shall | VALID | Holds (no elevation code exists) |

## 3. Validity findings

1. **§26 asks the user; the code decides.** §26 shows Rename / Skip / Cancel for
   an incompatible name. `build_manifest` repairs names automatically
   (`suggest_safe_name` plus a counter). That is non-destructive, as FR-NAME-002
   requires, but the user is never told. → D-F07-1.
2. **Unicode normalisation needs a dependency.** Rust's standard library has no
   NFC/NFD. `unicode-normalization` (an established crate) is needed for
   FR-NAME-001. → D-F07-2.
3. **Dropping permissions needs a recorded reason.** §25 says permissions
   *should* be preserved, and §114.2 forbids silently dropping a *should*. The code
   sends `mode: None` on purpose: permission bits from an unauthenticated peer —
   an executable bit in particular — are untrusted input. → D-F07-4.
4. **"Account for NTFS, APFS, ext4, btrfs" (FR-FS-003) is not testable.** →
   CR-12, with concrete cases: FAT32's 4 GiB file limit; case-insensitive NTFS
   and APFS; Windows path length; reserved device names (done).
5. **Symlinks are skipped silently (FR-FS-002).** Following them can loop or
   send files from outside the chosen folder, so skipping is correct. It must be
   visible. → D-F07-3.
6. **The symlink-escape test is vacuous on Windows.**
   `verify_within_rejects_symlink_escaping_the_root` puts its body under
   `#[cfg(unix)]`, so on the Windows development machine it passes without
   testing anything. CI's Linux runner does exercise it.
7. **Time-of-check/time-of-use.** A symlink swapped into the destination between
   `verify_within` and the write is not prevented; no portable API closes that
   window. The residual risk is another local process with write access to the
   user's chosen folder. Documented in `create_parent_dirs`; needs sign-off in
   the F08 security review.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F07-1 | Incompatible names | Repair automatically on the sender (implemented) and list every changed name on the sender's confirmation and in the receiver's completion summary. No per-file prompt. | Proposed |
| D-F07-2 | Unicode | Add `unicode-normalization`; normalise relative paths to NFC on the sender, compare NFC on the receiver when checking duplicates. | Proposed — new dependency |
| D-F07-3 | Symlinks | v1 never sends symlinks on any platform; the sender's summary says "N links were not sent". No sidecar in v1. | Proposed |
| D-F07-4 | Permissions | Not transferred (`mode: None`). Recorded justification: untrusted bits. Modification time is applied when it is between 1970 and 2100 (implemented). | Proposed |
| D-F07-5 | Conflicts | Per-transfer policy, default Keep Both; Replace requires a confirmation in the incoming dialog; Cancel aborts at finalisation if any destination exists. "Apply to all" is the per-transfer policy. | Proposed |
| D-F07-6 | Path length | Rust's standard library already adds the `\\?\` prefix for long absolute paths on Windows. Prove it with a test; refuse names over 255 bytes per component with a remedy. | Proposed |
| D-F07-7 | FAT32 | Pre-size each part file with `set_len` at open, so a 4 GiB+ file onto FAT32 fails at the start with "this drive cannot store files larger than 4 GB", not at 4 GB. | Proposed |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F07-01 | `..`, absolute, drive, UNC and device paths are rejected on every host OS | §91 | T | `filesystem::tests::safe_join_rejects_parent_traversal`, `…_absolute_posix_paths`, `…_windows_absolute_paths`, `…_unc_and_device_prefixes`, `…_drive_relative_component` | Passing |
| AC-F07-02 | Reserved device names, forbidden characters, control characters, trailing dot or space are rejected | §26 | T | `safe_join_rejects_reserved_device_names`, `…_windows_invalid_characters`, `…_control_characters`, `…_trailing_dot_or_space` | Passing |
| AC-F07-03 | A symlink inside the destination that points outside is refused | §91 | T (Linux CI) | `verify_within_rejects_symlink_escaping_the_root` | Passing on Linux; **vacuous on Windows** (finding 6) |
| AC-F07-04 | Two entries for one destination, case-only twins, and an entry inside a file are refused | §92 | T | `transfer::tests::prepare_rejects_two_entries_for_the_same_destination`, `…_names_that_differ_only_by_case`, `…_an_entry_inside_a_file` | Passing (uncommitted) |
| AC-F07-05 | Declared byte total and file count must match the entries; overflow is refused | §92 | T | `check_totals_rejects_a_lie_about_bytes_or_files`, `check_totals_survives_sizes_that_overflow` | Passing (uncommitted) |
| AC-F07-06 | A transfer id containing path characters is refused before any file is named after it | §92 | T | `filesystem::tests::a_transfer_id_with_path_characters_is_refused` | Passing (uncommitted) |
| AC-F07-07 | System folders and filesystem roots are refused as destinations, with a remedy | §49 | T | `system_folders_and_roots_are_dangerous_destinations`, `a_dangerous_root_is_refused_with_a_remedy` | Passing (uncommitted) |
| AC-F07-08 | Keep Both never overwrites; Replace does; Skip keeps the original | §24 | T | `finalising_with_keep_both_never_overwrites`, `finalising_with_replace_overwrites`, `finalising_with_skip_keeps_the_original_and_drops_the_part` | Passing (uncommitted) |
| AC-F07-09 | Every path the sender builds passes the receiver's `safe_join` | §26 | T | `every_manifest_path_passes_the_receivers_safe_join` | Passing (uncommitted) |
| AC-F07-10 | Given a hostile sender over loopback sending `../../x`, then the receiver refuses and nothing exists outside the root | §91 | T | planned: `session::tests::hostile_sender_cannot_escape_the_root` | Planned |
| AC-F07-11 | Given `é` as NFC on one side and NFD on the other, then one file results | FR-NAME-001 | T | planned: `filesystem::tests::nfc_and_nfd_are_one_name` | Planned (needs D-F07-2) |
| AC-F07-12 | Given a folder containing a symlink, then it is not sent and the sender is told | FR-FS-002 | T | planned: `filesystem::tests::skipped_links_are_reported` | Planned |
| AC-F07-13 | Given a 300-character path on Windows, then it is written | §26 | T (Windows CI) | planned: `filesystem::tests::long_windows_paths_work` | Planned |
| AC-F07-14 | Received files never carry the sender's permission bits | D-F07-4 | T | planned: `session::tests::permissions_are_not_applied` | Planned |
| AC-F07-15 | A file larger than 4 GiB onto a FAT32 drive fails at the start with a clear remedy | FR-FS-003 | D | Manual (USB stick) | Planned |

## 6. Out of scope

- Authenticating the sender → F08.
- Disk-space reporting → F11.

## 7. Current implementation

- [`filesystem.rs`](../src-tauri/src/filesystem.rs): `safe_join`,
  `verify_within`, `suggest_safe_name`, `ConflictResolution`, `unique_path`
  (committed); `build_manifest`, `claim_name`, `part_path`, `create_parent_dirs`,
  `finalize_file`, `apply_modified`, `map_write_error`, `check_destination_root`
  (uncommitted).
- [`transfer.rs`](../src-tauri/src/transfer.rs): `prepare` duplicate and
  ancestor checks, `check_totals` (uncommitted).
