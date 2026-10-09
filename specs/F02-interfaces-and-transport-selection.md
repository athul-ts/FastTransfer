# F02 — Interface enumeration and transport selection

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 2 (§108) |
| **SRS sources** | §11.2, §11.3, §16 (FR-TRANS-001…008), FR-DISC-007, §104, §101 |
| **Depends on** | F01 |
| **Verdict** | VALID |
| **Last validated** | 2026-11-09 |

## 1. Intent

Find every interface that can reach the other computer, rank them Thunderbolt →
Ethernet → Wi-Fi, connect over the best one, fall back when it fails, and always
show which one carried the transfer.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| FR-TRANS-001 | Enumerate usable interfaces | shall | VALID | Implemented, uncommitted (`transport::local_addrs`, `enumerate_interfaces`) |
| FR-TRANS-002 | Identify approximate interface type | shall | VALID | Implemented, uncommitted (`transport::classify`; macOS `en0` is a heuristic) |
| FR-TRANS-003 | Detect link capabilities where exposed | shall | VALID | Partial: Linux link speed from sysfs only |
| FR-TRANS-004 | Rank by preference, adjusted by measurement | shall | VALID | Partial: static ranking; measurements are never fed back |
| FR-TRANS-005 | Allow override in advanced settings | may | VALID | Not started |
| FR-TRANS-006 | Fall back when the preferred path fails | shall | VALID (see finding 1) | Not started |
| FR-TRANS-007 | A transport change must not corrupt completed data | shall | VALID | Not started (rests on F06 chunk atomicity) |
| FR-TRANS-008 | Always show the transport in use, in measured terms | shall | VALID | Not started |
| FR-DISC-007 | Never select virtual interfaces unless configured | shall | VALID | Implemented, uncommitted (`transport::is_virtual_name`) |

## 3. Validity findings

1. **FR-TRANS-006 is "shall" in §16 but listed as "should" in §114.2.** Same for
   FR-TRANS-005 ("may" in the text). The text is the stronger statement and
   matches §5.2 ("fall back… without degrading correctness"). → **CR-07**: fix the
   §114.2 table, keep the text.
2. **"Adjusted by measured performance" (FR-TRANS-004) needs a rule.** §104 shows
   Wi-Fi measured faster than Ethernet but does not say whether that reorders them.
   → D-F02-2.
3. **Interface type on Windows and macOS comes from names.** `if-addrs` exposes no
   interface-type field. The rule that keeps this honest: a Thunderbolt-looking
   name counts only when the F01 probe confirms a TB network (implemented). An
   Ethernet-vs-Wi-Fi misclassification only changes ranking, not a capability
   claim. → D-F02-3.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F02-1 | One transport type per medium, or one TCP transport for all? | One `TcpTransport` for every medium. Thunderbolt networking is an IP interface; only the label and the measurement differ. Keeps §11.2. | Accepted in code (uncommitted) |
| D-F02-2 | When does measurement override the class order? | Class order is fixed (TB > Ethernet > Wi-Fi). A measured throughput reorders *within* a class only. A slow Thunderbolt link still beats Ethernet unless the user overrides (FR-TRANS-005). | Proposed |
| D-F02-3 | How is a Windows interface's type found? | Names now (`Ethernet*`, `Wi-Fi`, `WLAN`); later `GetAdaptersAddresses` `IfType` (6 = Ethernet, 71 = 802.11), which also fixes the join described in §119.3. | Proposed |
| D-F02-4 | Fallback order when connecting | Try each peer endpoint ordered by the local interface's score; 5 s connect timeout each (`CONNECT_TIMEOUT`); stop at first success; report which one won. | Proposed |
| D-F02-5 | Socket tuning | Request 4 MiB send/receive buffers and `TCP_NODELAY` (implemented). Revisit in F09 with measurements. | Accepted in code (uncommitted) |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F02-01 | Every carrying medium maps to a transport; data-only and unknown media never do | FR-TRANS-002 | T | `transport::tests::every_carrying_medium_maps_to_a_transport`, `unusable_media_is_never_a_transport` | Passing |
| AC-F02-02 | Thunderbolt outranks Ethernet whatever Ethernet's rated speed; Ethernet outranks Wi-Fi | FR-TRANS-004 | T | `thunderbolt_outranks_ethernet_regardless_of_rated_speed`, `ethernet_outranks_wifi`, `speed_bonus_is_capped_below_the_next_class` | Passing |
| AC-F02-03 | VPN, container, hypervisor and tunnel adapters are flagged virtual and never become transports | FR-DISC-007 | T | `transport::tests::virtual_adapters_are_flagged_and_never_transports` | Passing (uncommitted) |
| AC-F02-04 | A Thunderbolt-named adapter with no probe evidence is not a transport | FR-LINK-003 | T | `a_thunderbolt_name_without_evidence_is_not_a_transport` | Passing (uncommitted) |
| AC-F02-05 | Loopback is never a candidate on the machine running the test | FR-TRANS-001 | T | `the_live_enumeration_never_reports_loopback` | Passing (uncommitted) |
| AC-F02-06 | With no usable interface, selection fails with `noTransport` and a remedy | FR-TRANS-006 | T | `no_interfaces_reports_no_transport` | Passing |
| AC-F02-07 | Given a peer with two endpoints where the first refuses, when sending, then the second is used and the transfer records which transport carried it | FR-TRANS-006, FR-TRANS-008 | T | planned: `session::tests::falls_back_to_the_next_endpoint` | Planned |
| AC-F02-08 | Given a completed transfer, then its measured throughput is stored and shown, never the link rating | FR-TRANS-004, §101 | T | planned: `transport::tests::measurement_feeds_ranking_within_a_class` | Planned |
| AC-F02-09 | Given two machines joined by a direct Ethernet cable and no router, when both apps run, then Ethernet is selected and reported | FR-TRANS-001, §73 | D | Manual | Planned — **feasible on the reference hardware** with an Ethernet adapter |
| AC-F02-10 | Given two TB-capable machines, then Thunderbolt is selected over Ethernet | FR-TRANS-004 | D | Manual | **CONDITIONAL (§5.2)** |

## 6. Out of scope

- Classifying the cable itself → F01.
- QUIC → F09 (§18.2 "evaluated").

## 7. Current implementation

- [`transport.rs`](../src-tauri/src/transport.rs): `TransportKind`,
  `score_for`, `candidates`, `select`, `TcpTransport` (connect with timeout,
  buffers, `TCP_NODELAY`), `bind_listeners` (dual-stack), `local_addrs`,
  `classify`, `group_interfaces`. Most of this is uncommitted work from this
  session.
- Missing: endpoint fallback loop, measurement feedback, user override, Windows
  `IfType`.
