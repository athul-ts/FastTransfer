# SRS validation report

| | |
|---|---|
| **Subject** | [`FastTransfer_SRS_v3.0.md`](../FastTransfer_SRS_v3.0.md), including amendments §119.1–§119.3 |
| **Date** | 2026-11-09 |
| **Code baseline** | Branch `ats`, working tree including uncommitted changes (marked "uncommitted") |
| **Method** | The SRS was read in full. Every identified requirement was checked for clarity, feasibility on supported hardware, testability, and consistency with the rest of the document, then compared with the code. |

## 1. Summary

### Is the main feature valid?

**Thunderbolt 3 / USB-C cable transfer is valid as a conditional capability, and
invalid as a guaranteed one.** The SRS already says this in §3.1 and §5.2, but
not consistently (DD-01).

- When both computers expose a Thunderbolt or USB4 network interface, the
  transfer is ordinary TCP over that interface. The engine needed (F03–F06) is
  the same one Ethernet uses, so building it is not wasted on any hardware.
- **On the reference pair it cannot work at all.** The Windows laptop's CPU
  contains a Thunderbolt block, but the firmware leaves it switched off, and
  the USB-C ports are 5 Gb/s USB 3.2 Gen 1 (§116.1.1, DD-21). Two USB hosts
  joined by a plain USB-C cable do not see each other, so no software change
  can make that cable carry files between these two machines.
- **What is valid and demonstrable now** is the guaranteed half of the
  objective: direct, router-free, verified transfer over the best path that
  exists — on this hardware, a direct Ethernet cable (USB Ethernet adapter if
  needed). Verifying the Thunderbolt half needs two machines that both have
  Thunderbolt 3/4 or USB4 host-to-host networking.

### What about laptops without a Thunderbolt controller?

**No software can give them Thunderbolt** (CR-20, investigated 2026-11-09).
Thunderbolt networking needs an enabled Thunderbolt or USB4 host controller at
*both* ends. No driver, app or cable can replace one. The reference Windows
laptop shows the point more sharply than §116.1.1 does. Its Core Ultra 7 255H
contains Intel's integrated Thunderbolt 4 block. But the laptop's firmware
leaves that block switched off, and ASUS built the USB-C ports for 5 Gb/s USB
only.

Eight routes were examined:

- **Adopted:** USB Ethernet adapters plus one Ethernet cable. The app already
  supports this.
- **Spike only:** driving a USB 3.0 bridge cable directly is worth a measured
  experiment, nothing more yet.
- **Rejected:** the rest, each with its reason, in
  [CR-20 in detail](#cr-20-in-detail-direct-cables-without-thunderbolt).

### Verdicts across the 85 identified requirements

| Verdict | Count | Meaning |
|---|---|---|
| VALID | 67 | Clear, feasible, testable as written |
| VALID WITH CHANGES | 11 | Needs a metric, clarification or decision |
| CONDITIONAL | 4 | Verifiable only on hardware the project lacks |
| NEEDS CHANGE | 3 | Cannot be implemented correctly as written: FR-LINK-002, FR-PAIR-003, NFR-SEC-001 |
| INVALID | 0 | — |

About 30 further binding statements carry no requirement ID at all (§5).

### Implementation status of the 85

| Status | Count |
|---|---|
| Implemented (tests pass; some uncommitted) | 12 |
| Partial | 34 |
| Not started | 32 |
| Holds by design or inspection (nothing to build yet) | 6 |
| Deferred by the SRS | 1 |
| **Verified** (SRS §117.2) | **0** — only the negative link case has been demonstrated (§119.3) |

### Five issues to resolve first

1. **Pairing cannot work as specified or as built** (CR-03, CD-02). Each side
   generates an unrelated random code, and a code compared after a plain Noise
   handshake is open to a man in the middle who tries keys until the codes agree.
   Needs a PAKE-style design and the R-03 security review.
2. **Encryption is guaranteed in §5.1 but missing from the MVP and loosely worded
   in NFR-SEC-001** (CR-04). The interim question is settled: **D-F08-7 accepted
   2026-11-09** — unencrypted transfers only in development builds, compiled out
   of release builds until F08 lands. CR-04's wording change is still open.
3. **Two of the five cable classes cannot be detected** on host-to-host links,
   yet four acceptance criteria require them (CR-02).
4. **The SRS still states Thunderbolt as the primary objective** in five places
   that §119.2 says were fixed (CR-01) — which also breaks FR-LINK-010.
5. **The IPC contract cannot express "accept this incoming transfer"**, which §48
   requires (CR-06).

## 2. Document defects

| ID | Where | Problem | CR |
|---|---|---|---|
| DD-01 | Front matter ("Primary Objective"), §1, §73, §110 ("Primary Transport"), §113 | Still state cable-speed Thunderbolt transfer as the primary objective; contradict §3, §3.1, §5. §119.2 claims this was fixed "anywhere"; it was not. | CR-01 |
| DD-02 | Revision history; §114.4 rule 1 | The 2026-10-09 amendments (§119.1–§119.3) changed the objective and assumptions but have no revision-history entry. The 3.0 entry still says "No requirement semantics changed". | CR-11 |
| DD-03 | §114.1 | Claims every requirement has an `FR-`/`NFR-` ID. About 30 binding statements do not (§5 below), so §118 cannot trace them. | CR-08 |
| DD-04 | §114.2 | Lists FR-TRANS-006 as *should* (text says *shall*), FR-TRANS-005 and FR-PAIR-007 as *should* (text says *may*). | CR-07 |
| DD-05 | §118, §116.2 | Wrong cross-references: "§89 Path traversal rejection" should be §91; R-04 cites "§35, §41" (Authentication, Transfer Completion) for diagnostics, should be §37, §43; R-10 cites "§50" (File Permissions), should be FR-RESUME-005 / §52. | CR-09 |
| DD-06 | §115.3 | FR-LINK-010 is defined in the assumptions section instead of §17, and is broken by the SRS's own text (DD-01). | CR-01 |
| DD-07 | §64 vs §100 | Small-file optimisation is *shall* (v1) in §64 and scheduled for v1.2 in §100. | CR-10 |
| DD-08 | FR-RESUME-004, §53, §106 | Restart recovery is "where practical", "where safe" and unconditional ("application restart recovers resumable transfers") in three places. | CR-14 |
| DD-09 | §102 | Sender flow has "Select destination", receiver flow "Choose destination"; the sender cannot choose a folder on another machine. The sender flow also has "Peer requests authorization" before the user selects files, which does not match FR-SESS-002. | CR-15 |
| DD-10 | §98, NFR-SEC-001 | MVP list omits encryption although §5.1 guarantees it; NFR-SEC-001 covers only "authenticated" transfers. | CR-04 |
| DD-11 | §79 | IPC list drifted from the code and lacks the incoming-transfer event and accept/reject commands §48 needs. | CR-06 |
| DD-12 | §67 vs CLAUDE.md | "All user-facing strings externalised" conflicts with the rule to build user-facing strings in `error.rs`. | CR-16 |
| DD-13 | NFR-PERF-001, -002, -007; FR-CHUNK-002, -004; FR-FS-003; §51 | No pass mark ("efficiently", "unnecessary", "saturate", "optimised", "excessive", "account for", "rotation and a size cap" with no size). | CR-12 |
| DD-14 | §115 | No assumption records that there is no macOS machine and no Thunderbolt-capable pair, so five §87 pairs and every macOS requirement are unverifiable. | CR-13 |
| DD-15 | FR-PAIR-003, NFR-SEC-009 | The code's origin is unspecified, and the comparison-code model over Noise XX needs a commitment step the SRS forbids building by hand. | CR-03 |
| DD-16 | FR-SESS-002 | Conflates the pairing initiator with the transfer sender; the tie-break is unnecessary if each transfer is its own connection. | CR-05 |
| DD-17 | §18.3, §93 | No minimum negotiated chunk size (a 1-byte chunk is a denial of service); no bound on manifest size beyond the 16 MiB frame. | CR-17, CR-18 |
| DD-18 | Repository root | `FastTransfer_SRS_v2.0.md` sits next to v3.0; both read as a baseline. | CR-19 |
| DD-19 | FR-LINK-002, §4.3 B–C, §88, §106 "Cable Link" bullets 3–4, §112 | `USB_DATA_ONLY` and `CHARGE_ONLY` are not observable on host-to-host links (§116.1.1: "Zero change"); the criteria are unreachable. | CR-02 |
| DD-20 | §117.2, R-02 | Status text is dated 2026-10-08 ("92 unit tests"); the suite is now 159. Cosmetic. | CR-11 |
| DD-21 | §116.1.1 | Says the Windows endpoint has no Thunderbolt controller and that "no OS-level or firmware-level change can create one". Inspection on 2026-11-09 shows otherwise. The CPU (Core Ultra 7 255H, Arrow Lake-H) integrates Thunderbolt 4, and its Type-C USB controller `8086:7EC0` is enabled. The Thunderbolt/USB4 host interfaces beside that controller are missing from the PCI bus, which means the firmware has disabled them; no Windows policy is involved (checked the same day, CR-20). The conclusion still holds; the stated reason is imprecise. | CR-20 |
| DD-22 | §5.2, §6, §88, §115 | Nothing says which direct cables are supported when Thunderbolt is absent. §6 does not rule out bridge cables, USB device mode, debug links or firmware modification. §88 has no row for USB Ethernet adapters. §115 records no assumption about the Windows endpoint's Ethernet port; it has none. | CR-20 |

## 3. Feature verdicts

| Feature | Verdict | Why | Blocking CRs |
|---|---|---|---|
| [F01](F01-cable-link-detection.md) Cable link detection | VALID WITH CHANGES | Honest classification works; two classes are undetectable; positive Thunderbolt case untestable here | CR-01, CR-02 |
| [F02](F02-interfaces-and-transport-selection.md) Interfaces and transport selection | VALID | Feasible; enumeration and ranking exist (uncommitted); fallback loop missing | — |
| [F03](F03-router-free-discovery.md) Router-free discovery | VALID | Guaranteed goal, demonstrable over direct Ethernet; nothing network-facing exists yet | — |
| [F04](F04-session-and-wire-protocol.md) Session and wire protocol | VALID WITH CHANGES | Framing is solid; role model ambiguous; no incoming-request IPC; auth depends on F08 | CR-05, CR-06, CR-17 |
| [F05](F05-file-and-folder-transfer.md) File and folder transfer | VALID | Building blocks exist; engine not written | CR-15 |
| [F06](F06-chunking-resume-integrity.md) Chunking, resume, integrity | VALID WITH CHANGES | Sound design; metrics and restart bar unclear | CR-12, CR-14 |
| [F07](F07-filesystem-safety.md) Filesystem safety | VALID WITH CHANGES | Path safety strong; Unicode, symlink warning, permissions decision open | CR-12 |
| [F08](F08-pairing-and-security.md) Pairing and security | **NEEDS CHANGE (blocking)** | Pairing design flaw; encryption scope; implementation incorrect | CR-03, CR-04 |
| [F09](F09-performance-and-limits.md) Performance and limits | CONDITIONAL | Targets need Thunderbolt or 10 GbE; three requirements lack a pass mark | CR-10, CR-12 |
| [F10](F10-desktop-ui.md) Desktop UI | VALID WITH CHANGES | IPC drift, strings conflict, state duplication | CR-06, CR-16 |
| [F11](F11-diagnostics-errors-logging.md) Diagnostics, errors, logging | VALID WITH CHANGES | Error model good; rotation unspecified; free space and Public-profile need platform APIs | CR-12 |
| [F12](F12-packaging-ci-release.md) Packaging, CI, release | VALID WITH CHANGES | CI lacks macOS Intel and security checks; no Mac for verification | CR-02, CR-13 |

## 4. Every identified requirement

Implementation marked * is uncommitted. "Holds" means the requirement is met
because nothing yet violates it, not because a feature implements it.

| ID | Requirement (short) | Kw | Verdict | Implementation | Spec |
|---|---|---|---|---|---|
| FR-DISC-001 | Discover peers on direct and local links | shall | VALID | Partial | F03 |
| FR-DISC-002 | mDNS/DNS-SD where available | shall | VALID | Not started | F03 |
| FR-DISC-003 | Must not depend on mDNS | shall | VALID | Not started | F03 |
| FR-DISC-004 | No DHCP needed; IPv4 + IPv6 link-local | shall | VALID | Not started | F03 |
| FR-DISC-005 | Advertisement fields | should | VALID | Partial | F03 |
| FR-DISC-006 | No automatic authorisation | shall | VALID | Holds | F03 |
| FR-DISC-007 | Skip virtual interfaces | shall | VALID | Implemented* | F02 |
| FR-DISC-008 | Update list on appear/disappear | shall | VALID | Not started | F03 |
| FR-DISC-009 | No file names in discovery | shall | VALID | Implemented | F03 |
| FR-DISC-010 | Rate-limit discovery | shall | VALID | Implemented | F03 |
| FR-PAIR-001 | Explicit approval of a new device | must | VALID | Partial | F08 |
| FR-PAIR-002 | Show name, OS, id, connection type | shall | VALID | Partial | F08 |
| FR-PAIR-003 | Short code on both endpoints must match | shall | NEEDS CHANGE | Partial — incorrect (CD-02) | F08 |
| FR-PAIR-004 | Single-use, time-limited codes | shall | VALID | Implemented | F08 |
| FR-PAIR-005 | Rate-limit pairing | shall | VALID | Partial — defect (CD-01) | F08 |
| FR-PAIR-006 | QR pairing later | should | VALID | Deferred (v1.1) | F08 |
| FR-PAIR-007 | Remember trusted devices | may | VALID | Not started | F08 |
| FR-PAIR-008 | Revoke trust | shall | VALID | Partial | F08 |
| FR-PAIR-009 | Secrets in OS storage | must | VALID | Not started | F08 |
| FR-TRANS-001 | Enumerate interfaces | shall | VALID | Implemented* | F02 |
| FR-TRANS-002 | Identify interface type | shall | VALID | Implemented* | F02 |
| FR-TRANS-003 | Detect link capabilities | shall | VALID | Partial | F02 |
| FR-TRANS-004 | Rank by preference and measurement | shall | VALID | Partial | F02 |
| FR-TRANS-005 | Manual override | may | VALID | Not started | F02 |
| FR-TRANS-006 | Fall back on failure | shall | VALID | Not started | F02 |
| FR-TRANS-007 | No corruption on transport change | shall | VALID | Not started | F02 |
| FR-TRANS-008 | Show transport in use | shall | VALID | Not started | F02, F10 |
| FR-LINK-001 | Detect whether a cable gives a network path | shall | CONDITIONAL | Implemented | F01 |
| FR-LINK-002 | Five-way classification | shall | NEEDS CHANGE | Partial | F01 |
| FR-LINK-003 | No false Thunderbolt claim | shall | VALID | Implemented | F01 |
| FR-LINK-004 | macOS bridge and status | shall | CONDITIONAL | Partial | F01 |
| FR-LINK-005 | Windows adapter and driver state | shall | VALID WITH CHANGES | Partial | F01 |
| FR-LINK-006 | Linux TB networking | shall | VALID | Implemented | F01 |
| FR-LINK-007 | Explain unusable links | shall | VALID | Implemented | F01 |
| FR-LINK-008 | Link speed ≠ measured speed | shall | VALID | Partial | F01 |
| FR-LINK-009 | Survive unplug; resume on replug | shall | VALID | Not started | F01, F06 |
| FR-LINK-010 | Never present TB as guaranteed | shall | VALID | Partial (CR-01) | F01 |
| FR-SESS-001 | Versioned protocol | must | VALID | Implemented | F04 |
| FR-SESS-002 | Explicit role model | shall | VALID WITH CHANGES | Not started | F04 |
| FR-SESS-003 | Reject mismatched versions on both sides | shall | VALID | Partial | F04 |
| FR-SESS-004 | Authenticate before file data | must | VALID | Not started | F04, F08 |
| FR-TRANSFER-001 | Select files | shall | VALID | Partial* | F05 |
| FR-TRANSFER-002 | Select folders | shall | VALID | Partial* | F05 |
| FR-TRANSFER-003 | Drag and drop | shall | VALID | Not started | F05 |
| FR-TRANSFER-004 | Preserve structure | shall | VALID | Partial* | F05 |
| FR-TRANSFER-005 | Stream contents | shall | VALID | Not started | F05 |
| FR-TRANSFER-006 | 100 GB+ tested | shall | VALID | Not started | F05 |
| FR-TRANSFER-007 | Progress | shall | VALID | Partial | F05 |
| FR-TRANSFER-008 | Cancel | shall | VALID | Partial* | F05 |
| FR-TRANSFER-009 | Pause and resume | should | VALID | Partial* | F05 |
| FR-TRANSFER-010 | Queue | shall | VALID | Not started | F05 |
| FR-CHUNK-001 | Chunk atomicity | shall | VALID | Partial | F06 |
| FR-CHUNK-002 | Configurable, per-transport chunk size | shall | VALID WITH CHANGES | Partial | F06 |
| FR-CHUNK-003 | Negotiated chunk size | shall | VALID WITH CHANGES | Implemented* | F06 |
| FR-CHUNK-004 | Low metadata overhead | shall | VALID WITH CHANGES | Not started | F06 |
| FR-RESUME-001 | Resumable | shall | VALID | Partial | F06 |
| FR-RESUME-002 | Receiver reports completed chunks | shall | VALID | Partial | F06 |
| FR-RESUME-003 | Send only missing chunks | shall | VALID | Partial | F06 |
| FR-RESUME-004 | Survive restart | shall | VALID WITH CHANGES | Not started | F06 |
| FR-RESUME-005 | Partial never shown complete | shall | VALID | Partial* | F06 |
| FR-RESUME-006 | Auto-resume on replug | may | VALID | Not started | F06 |
| FR-FS-001 | Unsupported metadata never fails a transfer | shall | VALID | Partial* | F07 |
| FR-FS-002 | Windows symlinks: sidecar or clear warning | shall | VALID | Partial — gap | F07 |
| FR-FS-003 | Account for NTFS, APFS, ext4, btrfs | shall | VALID WITH CHANGES | Not started | F07 |
| FR-NAME-001 | Unicode normalisation | shall | VALID | Not started | F07 |
| FR-NAME-002 | Non-destructive name resolution | shall | VALID WITH CHANGES | Partial* | F07 |
| FR-PAR-001 | Stream caps 4 / 16 | shall | VALID | Partial | F09 |
| FR-FW-001 | Windows "Public" profile | shall | VALID | Not started | F11 |
| NFR-PERF-001 | Use bandwidth efficiently | shall | VALID WITH CHANGES | Not started | F09 |
| NFR-PERF-002 | Avoid unnecessary copies | shall | VALID WITH CHANGES | Not started | F09 |
| NFR-PERF-003 | Stream large files | shall | VALID | Not started | F09 |
| NFR-PERF-004 | Report measured throughput | shall | VALID | Partial | F09 |
| NFR-PERF-005 | Multi-GB/s where possible | should | CONDITIONAL | Not started | F09 |
| NFR-PERF-006 | No fixed ceiling | shall | VALID | Holds | F09 |
| NFR-PERF-007 | Saturate a Thunderbolt link | shall | CONDITIONAL | Not started | F09 |
| NFR-SEC-001 | Authenticated transfers encrypted | shall | NEEDS CHANGE | Not started | F08 |
| NFR-SEC-002 | Discovery grants no access | shall | VALID | Holds | F08 |
| NFR-SEC-003 | Explicit authorisation | shall | VALID | Partial | F08 |
| NFR-SEC-004 | Established crypto libraries | shall | VALID | Holds (see CD-05) | F08 |
| NFR-SEC-005 | Keys in OS secure storage | shall | VALID | Not started | F08 |
| NFR-SEC-006 | No file data before authorisation | shall | VALID | Not started | F08 |
| NFR-SEC-007 | No file contents in logs | shall | VALID | Holds | F11 |
| NFR-SEC-008 | No keys in logs | shall | VALID | Holds | F11 |
| NFR-SEC-009 | Documented handshake from a library | shall | VALID WITH CHANGES | Not started | F08 |
| NFR-A11Y-001 | WCAG 2.1 AA | shall | VALID | Partial | F10 |

## 5. Binding statements without an ID

§114.1 requires an ID for every requirement. These *shall/must* statements have
none. CR-08 proposes the IDs below; none may reuse an existing number.

| Section | Statement (short) | Proposed ID | Spec |
|---|---|---|---|
| §23 | Hash with SHA-256; verify after transfer; stream hashing; failure marks failed | FR-INTEG-001…003 | F06 |
| §24 | Keep Both default; destructive overwrite needs confirmation | FR-CONFLICT-001…002 | F07 |
| §25 | Support regular files and directories | FR-FS-004 | F07 |
| §26 | Detect incompatible names, length limits, case, normalisation | FR-NAME-003 | F07 |
| §29 | Sender must slow down when the receiver cannot keep up | NFR-PERF-008 | F09 |
| §32 | Memory independent of file size; bounded, configurable buffers | NFR-MEM-001…002 | F09 |
| §33 | No blind compression | NFR-CPU-001 | F09 |
| §37 | Explain firewall problems; never disable the firewall | FR-FW-002…003 | F11 |
| §45 | Default download location | FR-SET-001 | F05 |
| §46 | History holds metadata only | FR-HIST-001 | F10 |
| §48 | Receiver can reject | FR-INCOMING-001 | F05 |
| §49 | Refuse dangerous destinations | FR-DEST-001 | F07 |
| §50 | Never silently elevate | FR-DEST-002 | F07 |
| §51 | Log levels, `INFO` default, rotation and size cap | NFR-LOG-001…002 | F11 |
| §52 | A crash never marks an incomplete transfer complete | FR-REL-001 | F06 |
| §53 | Restore state after restart; show incomplete transfers | FR-REL-002 | F06 |
| §64 | Small-file optimisation | NFR-PERF-009 | F09 |
| §65 | ETA from a moving window | FR-PROG-001 | F09 |
| §67 | Externalised strings | NFR-I18N-001 | F10 |
| §69 | UI does not block during discovery | NFR-START-001 | F10 |
| §71 | Privacy prohibitions | NFR-PRIV-001…005 | F11 |
| §72 | Offline operation | NFR-OFF-001 | F03 |
| §73 | No-router, no-Internet direct scenario | FR-DIRECT-001 (reworded per CR-01) | F03 |
| §80 | Rust is the source of truth | NFR-ARCH-001 | F10 |
| §81 | Minimal Tauri capabilities | NFR-SEC-010 | F10 |
| §85 | CI builds four targets | NFR-CI-001 | F12 |
| §91 | Path traversal protection | NFR-SEC-011 | F07 |
| §92 | Treat all remote metadata as untrusted | NFR-SEC-012 | F07 |
| §95 | Every error states what, why, what to do | FR-ERR-001 | F11 |
| §101 | Never advertise speed from the link rating | FR-LINK-011 | F01 |

## 6. Change requests to the SRS

Each is applied under §114.4. "Review" marks a change that needs a recorded
security review.

| ID | Priority | Change | Sections |
|---|---|---|---|
| CR-01 | High | Restate the objective in the front matter, §1, §73, §110 and §113 to match §3.1 (guaranteed: direct, router-free, verified, encrypted transfer over the best path; conditional: Thunderbolt). Move FR-LINK-010 into §17. Correct §119.2's claim. | Front matter, §1, §17, §73, §110, §113, §115.3, §119.2 |
| CR-02 | High | FR-LINK-002: `USB_DATA_ONLY` and `CHARGE_ONLY` are reported **where the OS can observe them**; otherwise `NONE` with a hedged explanation. Rewrite §4.3 B–C, §88 cable rows, §106 bullets 3–4 and §112 accordingly. | §4.3, §17, §88, §106, §112 |
| CR-03 | **Blocking** — Review | FR-PAIR-003: the code is shown on one endpoint and entered on the other, and authenticates the key exchange through a PAKE from an established library; independent per-device codes and hand-built commitment steps are prohibited. Amend NFR-SEC-009 to allow PAKE + Noise. | §15, §34 |
| CR-04 | **Blocking** — Review | NFR-SEC-001: "All transfers shall be authenticated and encrypted." State whether the MVP (§98) may ship without encryption; if not, add encryption to the MVP list. | §34, §98 |
| CR-05 | Medium | FR-SESS-002: the role is per connection; the connecting side is the sender. Remove the simultaneous-initiation tie-break or limit it to pairing. | §19 |
| CR-06 | High | §79: add `incoming_transfer` event, `accept_transfer`, `reject_transfer`, `confirm_pairing`, `get_trusted_devices`, `list_transfers`; drop `select_files` (the dialog plugin covers it); keep `retry_transfer` and `get_transfer_history` as planned. | §79 |
| CR-07 | Low | Fix the §114.2 table for FR-TRANS-005, FR-TRANS-006, FR-PAIR-007. | §114.2 |
| CR-08 | Medium | Assign IDs to the §5 statements; add them to §118. | many |
| CR-09 | Low | Fix the cross-references in DD-05. | §116.2, §118 |
| CR-10 | Medium | Decide whether §64 is v1 (*shall*) or v1.2 (§100). | §64, §100 |
| CR-11 | Low | Add revision-history entries for 2026-10-09 (consider 3.1); refresh §117.2 and R-02 status text. | front matter, §116, §117.2 |
| CR-12 | Medium | Add pass marks: NFR-PERF-007 (≥ 80 % of min(link TCP throughput, storage write speed)); NFR-PERF-001/-002 (benchmark and inspection criteria); FR-CHUNK-002 (default and bounds); FR-CHUNK-004 (≤ 1 bit per chunk + O(files)); FR-FS-003 (named cases); §51 (5 MiB × 3). | §21, §25, §30, §51 |
| CR-13 | Medium | Add assumptions: no macOS reference machine; no Thunderbolt-capable pair. Mark the affected §87/§88 rows. (CR-20 covers the Windows laptop's Ethernet port; it has none.) | §115, §87, §88 |
| CR-14 | Medium | Pick one bar for restart recovery across FR-RESUME-004, §53 and §106. | §22, §53, §106 |
| CR-15 | Low | §102: sender chooses the peer; receiver's folder is its setting (§45). Fix the pairing step order. | §102 |
| CR-16 | Medium | Resolve §67 vs CLAUDE.md: the core returns `kind` + parameters; the UI owns all text. Update CLAUDE.md in the same change. | §67, CLAUDE.md |
| CR-17 | Medium — Review | Chunk size bounds: 64 KiB to 16 MiB. | §18.3, §21, §93 |
| CR-18 | Low | Manifest ceiling: 100 000 entries per transfer in v1 with a clear refusal; paged manifests later. | §18.4, §64, §93 |
| CR-19 | Low | Move `FastTransfer_SRS_v2.0.md` out of the repository root (git history keeps it). | repository |
| CR-20 | High | Direct cables when Thunderbolt is absent. The SRS should state that no software can provide Thunderbolt without a controller at both ends, and name direct Ethernet (including USB Ethernet adapters) as the cable path. Also: list the rejected routes in §6; add rows to §88; add assumption A-08 and dependency D-08; correct the reason given in §116.1.1; make FR-LINK-007's `NONE` remedy name the supported alternative. Details below. | §5.2, §6, §17, §88, §115, §116.1.1 |

### CR-20 in detail: direct cables without Thunderbolt

Raised 2026-11-09, when the owner asked whether a laptop without a Thunderbolt
controller can transfer files over Thunderbolt in any way.

**What the reference Windows laptop actually has.** Read-only inspection on
the machine itself, 2026-11-09:

| Observation | Result |
|---|---|
| Model, firmware | ASUS Vivobook S 14 S3407CA, BIOS S3407CA.303 (2025-03-03) |
| CPU | Intel Core Ultra 7 255H (Arrow Lake-H). Intel ships this family with integrated Thunderbolt 4 |
| Type-C USB controller | `8086:7EC0` at 00:0D.0, enabled. It belongs to the chip's Type-C subsystem, which serves only USB-C ports |
| Thunderbolt/USB4 host interfaces | Absent. On this platform they would be `8086:7EC2` and `7EC3` (the Meteor Lake-P IDs in Linux `nhi.h`). No Thunderbolt PCIe root ports appear either |
| USB device controller (xDCI) | Absent. Linux `dwc3-pci` lists `8086:777E` and `7EC1` for this platform; neither appears. The laptop therefore cannot act as a USB device |
| Type-C port management | UCSI (`ACPI\USBC000`); ports rated USB 3.2 Gen 1, 5 Gb/s |
| Network adapters | Wi-Fi only (Realtek 8852BE); no Ethernet port |
| Company management | The laptop is Entra ID joined and managed by Intune. Removable storage is set to deny all access, which is why USB drives fail. **Not set:** device-installation restrictions (the mechanism Microsoft documents for blocking Thunderbolt controllers) and Defender device control (off). The USB4 drivers are enabled (`Usb4HostRouter` Start=3). Kernel DMA Protection's enumeration policy is the Windows default, "only after log in/screen unlock". Windows has never seen a Thunderbolt (`CC_0C0A`) or USB4 (`CC_0C0340`) device on this laptop, and no device is in an error state |

The chip contains the Thunderbolt block, but the firmware and the board do not
use it. **Company policy is not the cause.** Windows policies act on drivers
and access after the hardware has been found. A policy-blocked controller still
appears in the device list, carrying an error. Here the controller never
appears at all. ASUS rates the ports at 5 Gb/s, below what the chip's Type-C block can
do. That points to limits on the board itself — the signal path and the
power-delivery controller's firmware, which are exactly the parts Thunderbolt
depends on. *This last point is an inference, not a measurement.*

**Routes examined**

| # | Route | On the reference pair | Ceiling | Work for FastTransfer | Verdict |
|---|---|---|---|---|---|
| 1 | Thunderbolt/USB4 networking at both ends | Not possible with the Windows laptop. The Linux endpoint can take part once Thunderbolt is enabled in its firmware, paired with another Thunderbolt or USB4 machine | 20–40 Gb/s link | None: F01/F02 already detect and prefer it | **Keep** as the conditional headline (§5.2) |
| 2 | USB Ethernet adapter(s) plus one Ethernet cable, no router | Yes. Windows needs an adapter; Linux has onboard 1 GbE | 1 GbE ≈ 117 MB/s; 2.5 GbE ≈ 290 MB/s. Any USB path on the Windows laptop stays under ~500 MB/s (a 5 Gb/s port with 8b/10b encoding) | None: names such as `Ethernet 3` and `enx…` already classify as Ethernet (AC-F02-11) | **Adopt** as the guaranteed cable path and the reference rig |
| 3 | USB 3.0 bridge cable in network mode (Prolific PL27A1 with RNDIS firmware, ID `067B:27A5`) | Unlikely. No retail cable with this firmware was found, and Plugable says it does not have it. Linux maintainers are disabling the RNDIS drivers in 2026 for security reasons | ≤ 5 Gb/s signalling; unmeasured | Classify `usb0`-style interfaces | **Reject** |
| 4 | USB 3.0 bridge cable driven directly (retail PL27A1, ID `067B:27A1`). Windows uses the built-in WinUSB driver; Linux uses user-space USB after detaching `plusb` | Plausible; unverified | ≤ 5 Gb/s signalling; unmeasured | A new non-IP transport that carries the session byte stream over USB bulk pipes. Also: peer detection over the cable, a USB library, a Linux udev rule | **Spike only**: measure before deciding (D-CR20-1) |
| 5 | One laptop acts as a USB network device (NCM gadget) | No. The Windows laptop has no USB device controller, and Windows device mode needs one | ≤ 5 Gb/s | Linux-only setup | **Reject** for v1 |
| 6 | xHCI Debug Capability over a USB 3.0 A-to-A debugging cable | Lab only. Linux must be the debug target, and Windows needs a custom driver binding. The feature is built for kernel debugging and does not support runtime suspend | Unmeasured | A custom transport | **Reject** |
| 7 | Turning on the CPU's disabled Thunderbolt block through hidden firmware settings | ASUS does not support it, and the port hardware is built for 5 Gb/s. Editing firmware variables can leave a laptop unable to boot | — | — | **Reject; never recommend** |
| 8 | Wi-Fi, with one laptop hosting a hotspot (no router) | Yes | Far below any cable | None (this is already "Wi-Fi") | **Keep** as the fallback |

No vendor offers Thunderbolt transfer without Thunderbolt hardware. Intel's own
PC-to-PC product, Thunderbolt Share, needs Thunderbolt 4 or 5 on both PCs.

**Proposed SRS changes**

1. §5.2: add "Where either computer lacks Thunderbolt or USB4 host-to-host
   networking, no software can provide it. The direct-cable path is then
   Ethernet, including USB Ethernet adapters."
2. §6 (non-goals): add modifying firmware or enabling hidden hardware, USB
   device (gadget) mode, xHCI debug links and RNDIS bridge cables. Driving a
   bridge cable directly also stays out of scope unless the D-CR20-1 spike is
   accepted.
3. §88: add two rows: "Direct Ethernet through USB Ethernet adapter(s), no
   router, no DHCP"; "Windows USB4 ↔ Linux Thunderbolt 3 host-to-host
   (interoperability)".
4. §115.1: add **A-08**. The Windows endpoint has no Ethernet port, so a direct
   cable needs a USB Ethernet adapter, and its 5 Gb/s USB port caps every USB
   path. §115.2: add **D-08**, USB 3.x 2.5 GbE Ethernet adapter(s).
5. §116.1.1: replace "no controller … no firmware-level change can create one"
   with the inspection results above (DD-21). The conclusion does not change.
6. FR-LINK-007: when the class is `NONE`, the explanation names the supported
   alternative, a USB Ethernet adapter and an Ethernet cable. It never suggests
   changing firmware.

**Decision**

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-CR20-1 | Should route 4, a USB bridge-cable transport driven directly, be pursued? | Only as a time-boxed spike. Use one PL27A1 cable and a throwaway program that streams bulk data Windows ↔ Linux and records MB/s. Build it into FastTransfer only if it beats route 2 on the reference pair, and only if the owner accepts the new transport, its dependency and the permissions it needs. Label it "USB bridge cable", never Thunderbolt (FR-LINK-003). Plugging the cable in never implies trust; F08 applies unchanged. Learn the cable's control requests from documentation or bench observation; do not copy GPL driver code. | Proposed |

**Hardware actions** (these are not SRS changes):

- Buy one or two USB 2.5 GbE adapters, for AC-F02-09, AC-F05-13 and AC-F09-09.
- Enable Thunderbolt in the Linux endpoint's firmware, then check `boltctl` and
  `/sys/bus/thunderbolt`.
- Borrow a Thunderbolt 4 or USB4 machine for the CONDITIONAL criteria.

**Sources:**
- [Intel Core Ultra 200H/200U quick reference](https://cdrdv2-public.intel.com/842536/Intel%20Core%20Ultra%20200H%20and%20200U%20Series%20Processors%20-%20Quick%20Reference%20Guide%20v1.1.pdf)
- [Notebookcheck: Core Ultra 7 255H](https://www.notebookcheck.net/Intel-Core-Ultra-7-255H-Processor-Benchmarks-and-Specs.944139.0.html)
- [Linux `nhi.h`](https://codebrowser.dev/linux/linux/drivers/thunderbolt/nhi.h.html)
- [Linux `dwc3-pci.c`](https://kernelsources.org/source/xref/linux/drivers/usb/dwc3/dwc3-pci.c?a=true)
- [Prolific PL27A1 datasheet](https://www.prolific.com.tw/wp-content/uploads/2025/07/DS-27131201-PL27A1_V1.5.pdf)
- [Plugable: USB 3.0 RNDIS mode](https://support.plugable.com/t/usb-3-0-rndis-mode/16372)
- [Linux `plusb` PL-27A1 patch](https://lore.kernel.org/all/56C9CC84.7040705@gmail.com/T/)
- [Phoronix: Linux disabling RNDIS drivers](https://www.phoronix.com/news/RNDIS-Disable-Linux-2026-Patch)
- [Linux USB3 debug port](https://docs.kernel.org/driver-api/usb/usb3-debug-port.html)
- [Microsoft NCM driver](https://github.com/microsoft/NCM-Driver-for-Windows)
- [Linux USB4 and Thunderbolt](https://docs.kernel.org/admin-guide/thunderbolt.html)
- [Microsoft USB4 connection manager](https://learn.microsoft.com/en-us/windows-hardware/design/component-guidelines/usb4-intro-to-connection-manager)
- [Intel Thunderbolt Share requirements](https://intel.com/content/www/us/en/support/articles/000099003/software.html)

## 7. Code defects found while validating

| ID | Where | Problem | Requirement | Severity |
|---|---|---|---|---|
| CD-01 | `security.rs` `PairingState` | `PAIRING_RATE_WINDOW` (300 s) is never used; the attempt counter resets when the 120 s code expires. `docs/api-reference.md` claims five minutes. | FR-PAIR-005 | High |
| CD-02 | `security.rs` `generate_pairing_code`, `commands.rs` `pair_device` / `confirm_pairing` | Each device generates its own random code; the typed code is compared with this machine's code, not the peer's. Pairing authenticates nothing. | FR-PAIR-003 | High |
| CD-03 | `commands.rs` `AppState::new`, `confirm_pairing`; `security.rs` `save_trusted` | Trusted list starts empty, is never filled, never persisted. | FR-PAIR-007, -008 | Medium |
| CD-04 | `App.tsx` vs `commands.rs` | UI listens for `link_changed` and `device_*`; the core never emits them. | FR-DISC-008, FR-LINK-009 | Medium |
| CD-05 | `security.rs` `constant_time_eq` | Hand-written constant-time comparison; use `subtle`. | NFR-SEC-004 | Low |
| CD-06 | `discovery.rs` `Advertisement::to_txt_pairs` | Advertisement lacks transport types and link capability. | FR-DISC-005 | Low |
| CD-07 | `commands.rs` `ApplicationSettings::default`, `get_settings` | Download directory defaults to empty; settings are never persisted; the UI keeps its own copy in `localStorage`. | §45, §80 | Medium |
| CD-08 | `.github/workflows/ci.yml` | No macOS Intel runner; no security-check stage; bundle job only on manual dispatch. | §85 | Medium |
| CD-09 | `lib.rs` `init_tracing` | No log file, rotation or size cap. | §51 | Low |
| CD-10 | `protocol.rs` `Hello` / `HelloAck` | Nonce echo is a hand-built handshake step; it must be replaced by the F08 Noise session. | NFR-SEC-009 | Medium (dormant) |
| CD-11 | `types/index.ts` | `TransferProgress` lacks `direction`, `peerName`, `error`, which the Rust struct gained in uncommitted work — the TS mirror is out of step. | CLAUDE.md mirror rule | Medium (uncommitted) |
| CD-12 | `filesystem.rs` `verify_within_rejects_symlink_escaping_the_root` | Test body is `#[cfg(unix)]`; it passes without testing on Windows. | §91 | Low |

## 8. What can be verified on the current hardware

| Can verify now | Cannot verify now |
|---|---|
| Everything marked **T** (loopback tests) | Any positive Thunderbolt or USB4 criterion (no capable pair) |
| Windows ↔ Linux over a direct Ethernet cable, with a USB Ethernet adapter on the Windows laptop (CR-20): discovery, transfer, resume, unplug/replug (AC-F02-09, AC-F03-10, AC-F05-13, AC-F06-12) | macOS anything (no Mac): FR-LINK-004, five of nine §87 pairs |
| 2.5 GbE throughput, with USB adapters at both ends (AC-F09-09) | Windows USB4 ↔ Linux Thunderbolt 3 interoperability (needs a USB4 Windows machine, and Thunderbolt enabled on the Linux endpoint) |
| Honest `NONE` classification with the USB-C cable connected (done, §119.3) | `USB_DATA_ONLY` / `CHARGE_ONLY` (not observable; CR-02) |
| Windows "Public" profile note (AC-F11-06) | Multi-GB/s throughput (NFR-PERF-005, -007) |
| FAT32 4 GiB behaviour with a USB stick (AC-F07-15) | Signed installers (D-05, D-06 not acquired) |

## 9. Recommended order

1. **Owner review** of CR-01 to CR-04 — they decide what "done" means. (D-F08-7
   was accepted on 2026-11-09.)
2. **Approve F03, F04, F05, F06 and F07.** They are valid now, testable on
   loopback and demonstrable over direct Ethernet. Implement them tests-first.
3. **Get the cable hardware** (CR-20): a USB 2.5 GbE adapter for the Windows
   laptop (ideally one per machine), Thunderbolt enabled in the Linux
   endpoint's firmware, and a borrowed Thunderbolt 4 or USB4 machine. Without
   it, no D criterion on a cable can run.
4. **Security review of F08**, then implement it — before any release build can
   transfer files.
5. **Finish F01 and F02**: link watcher, fallback loop, measurement feedback.
6. **F09–F12.**
7. Re-validate this report at the end of each phase (§116.3).
