# F01 — Cable link detection

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 2 (§108) |
| **SRS sources** | §4, §5.2, §17 (FR-LINK-001…009), §115.3 (FR-LINK-010), §101, §106 "Cable Link", §88, §116.1.1, §119.3 |
| **Depends on** | — |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

Tell the user, truthfully, what the plugged cable can do. When both computers
expose a Thunderbolt-backed network interface, say "Thunderbolt" and let F02
prefer it. Otherwise say what is missing and fall back. A wrong `THUNDERBOLT`
claim is the one failure this product must not have.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| FR-LINK-001 | Determine whether a cable exposes a usable network path | shall | CONDITIONAL — the negative case is verifiable now | Implemented; negative case demonstrated on the Windows reference machine (§119.3) |
| FR-LINK-002 | Classify into `THUNDERBOLT`, `USB4_NETWORK`, `USB_DATA_ONLY`, `CHARGE_ONLY`, `NONE` | shall | **NEEDS CHANGE** — see finding 1 | Partial: `USB_DATA_ONLY` is never produced |
| FR-LINK-003 | Never report `THUNDERBOLT` without a TB-backed IP interface | shall | VALID | Implemented (`link::classify`, device-tree probe on Windows) |
| FR-LINK-004 | macOS: detect Thunderbolt Bridge and its status | shall | CONDITIONAL — no Mac to verify on (CR-13) | Partial: name match on `bridge0`, no status |
| FR-LINK-005 | Windows: detect TB adapter and its driver state | shall | VALID WITH CHANGES — "driver state" undefined (finding 4) | Partial: device-bound detection; driver *state* not reported |
| FR-LINK-006 | Linux: detect TB networking support and interface | shall | VALID | Implemented (sysfs probe, synthetic-tree tests) |
| FR-LINK-007 | Explain an unusable link in user language | shall | VALID | Implemented (`link::explain`) |
| FR-LINK-008 | Keep link speed and measured speed distinct | shall | VALID | Partial: types are distinct; measured value never populated |
| FR-LINK-009 | Survive unplug/replug; attempt resume on replug | shall | VALID | Not started (no link monitoring) |
| FR-LINK-010 | Never present Thunderbolt as guaranteed | shall | VALID | Partial: README and bundle text fixed (uncommitted); the SRS front matter, §1, §73, §110 and §113 still violate it (CR-01) |

## 3. Validity findings

1. **Two of the five classes cannot be observed on host-to-host links.**
   `USB_DATA_ONLY` and `CHARGE_ONLY` (FR-LINK-002, §4.3 cases B and C, §88, §106
   acceptance bullets 3–4, §112 "USB data-only cable must classify correctly")
   assume the OS reports something when a non-Thunderbolt USB-C cable joins two
   computers. On the reference hardware it reports nothing: "Effect of inserting
   the cable — **Zero change**" (§116.1.1). Two USB hosts joined by a plain USB-C
   cable do not enumerate each other. Only Linux, and only where the Type-C class
   is exposed, can see a partner, and even then cannot tell data from
   charge-only. The code is honest about this (`usb_data: false` in every probe,
   documented in `link::observe`). As written, these acceptance criteria are
   unreachable — the same trap §119.2 removed for the Thunderbolt case, and they
   tempt an implementer to guess. → **CR-02**: keep the classes, but make them
   "where the OS can observe it", and accept `NONE` with a hedged explanation
   otherwise.
2. **The positive Thunderbolt case cannot be demonstrated on the reference
   hardware.** The Windows endpoint has no usable Thunderbolt or USB4 controller:
   its CPU's Thunderbolt block is disabled by firmware (§116.1.1, finding 5).
   This is already recognised (§5.2, §99 `~`). Every positive-case criterion below
   is `CONDITIONAL (§5.2)`.
3. **FR-LINK-010 lives in §115.3**, not with the other FR-LINK requirements in
   §17, and the SRS breaks it in its own front matter ("Primary Objective: …over a
   Thunderbolt 3 / USB-C cable"), §1, §73, §110 ("Primary Transport: Thunderbolt")
   and §113. §119.2 says this was fixed "anywhere"; it was not. → **CR-01**.
4. **FR-LINK-005 "driver state"** has no defined states. The probe can say
   "driver bound / driver present but stopped / absent" (the §116.1.1 table shows
   `Usb4HostRouter` "Present but Stopped"). → D-F01-2.
5. **No software can provide Thunderbolt unless both ends have a controller.
   On the reference laptop the controller is present but disabled.** A
   re-inspection on 2026-11-09 found that the Windows laptop's CPU (Core Ultra 7
   255H) integrates Thunderbolt 4. The firmware leaves the Thunderbolt/USB4 host
   interfaces off, and the board's USB-C ports are rated 5 Gb/s. `NONE` remains
   the correct class; only the reason the SRS gives is wrong (DD-21).
   → **CR-20**.
6. **USB-based alternatives are network interfaces, not cable classes.** A USB
   Ethernet adapter, a bridge cable or a USB network device appears as an
   ordinary interface, and F02 handles it. None of them may ever be reported as
   `THUNDERBOLT` or `USB4_NETWORK` (FR-LINK-003).

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F01-1 | What to show when a USB-C cable is plugged in but the OS sees nothing? | `NONE`, with the hedged text "No usable cable connection detected. If a cable is plugged in, it does not provide a network path between these computers." Never guess `USB_DATA_ONLY`. | Proposed |
| D-F01-2 | What does "driver state" mean on Windows? | Report one of `bound`, `installed-not-running`, `blocked`, `absent` for `Usb4HostRouter` / `Usb4P2PNetAdapter` / Thunderbolt class drivers, from SetupAPI. `blocked` means the device is present but carries a problem code. One cause is an IT administrator's device-installation restriction on Thunderbolt (`PCI\CC_0C0A`), as Microsoft documents for BitLocker hardening. The state is shown in Diagnostics (F11). `blocked` also appears in the banner, because it is the one state the user can act on: "Your organisation's policy blocks the Thunderbolt controller; ask your IT administrator." The probe ignores problem codes today. | Proposed |
| D-F01-3 | How is a plug/unplug noticed (FR-LINK-009)? | Poll `link::current()` every 2 s on a blocking thread and emit `link_changed` only on change. OS notifications (WM_DEVICECHANGE, udev, IOKit) are a later optimisation. | Proposed |
| D-F01-4 | macOS bridge status | `bridge0` present and `UP` with an address → usable; present but down → explain "Thunderbolt Bridge is off". Needs a Mac to verify. | Proposed |
| D-F01-5 | What does the `NONE` explanation recommend? | Name the supported alternative: "For a direct cable between these computers, use a USB Ethernet adapter and an Ethernet cable." Never suggest firmware changes or unsupported cables (CR-20). | Proposed |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F01-01 | Given a controller that supports Thunderbolt but no TB network interface, then the class is never `THUNDERBOLT` | FR-LINK-003 | T | `link::tests::usb_c_connector_alone_is_not_thunderbolt` | Passing |
| AC-F01-02 | Given an interface renamed to look like Thunderbolt with no device-tree backing, then it is not classified Thunderbolt | FR-LINK-003 | T | `link::tests::ordinary_interfaces_are_never_thunderbolt`, `transport::tests::a_thunderbolt_name_without_evidence_is_not_a_transport` | Passing (second test uncommitted) |
| AC-F01-03 | Given a USB4 host router, then the class is `USB4_NETWORK`, not `THUNDERBOLT` | FR-LINK-002 | T | `link::tests::usb4_host_classifies_as_usb4_not_thunderbolt` | Passing |
| AC-F01-04 | Given no sysfs Thunderbolt bus, then the class is `NONE` | FR-LINK-006 | T | `link::tests::absent_sysfs_reports_no_cable` | Passing |
| AC-F01-05 | Given any unusable class, then the explanation has a non-empty detail and a remedy | FR-LINK-007 | T | `link::tests::unusable_links_always_offer_a_remedy` | Passing |
| AC-F01-06 | Given the Windows reference machine with the USB-C cable connected, when the probe runs, then it reports `NONE` with `thunderbolt_capable: false` | FR-LINK-001, FR-LINK-003 | D | Manual, recorded in SRS §119.3 | Demonstrated 2026-10-09. Hardware re-inspected 2026-11-09 (CR-20): the CPU's Thunderbolt block is disabled by firmware, so `false` is still correct |
| AC-F01-07 | Given the app is running, when the link class changes, then `link_changed` is emitted within 3 s and the banner updates | FR-LINK-009 | T + D | planned: `commands::tests::link_watcher_emits_only_on_change` | Planned |
| AC-F01-08 | Given an active transfer, when the cable is unplugged, then the app does not crash and the transfer becomes `INTERRUPTED` | FR-LINK-009 | T (loopback drop) + D | `session::tests::partial_chunk_is_never_counted` (engine ends with resumable `connectionLost`; the orchestrator maps it to `INTERRUPTED`) | Passing (uncommitted); D pending |
| AC-F01-09 | Given two machines that both expose a TB network interface, when cabled, then both classify `THUNDERBOLT` | FR-LINK-001 | D | Manual, SRS §87–§88 | **CONDITIONAL (§5.2)** — no hardware |
| AC-F01-10 | Given any UI surface showing the link, then it shows the link rating and the measured speed as separate, labelled values | FR-LINK-008, §101 | I + T | planned: `LinkBanner.test.tsx` | Planned |
| AC-F01-11 | Given the repo's user-facing text (README, bundle metadata, UI strings, SRS front matter), then none presents Thunderbolt as guaranteed | FR-LINK-010 | I | Review checklist | Partial (CR-01 open) |
| AC-F01-12 | Given class `NONE`, then the remedy names a USB Ethernet adapter and an Ethernet cable, and mentions no firmware change | FR-LINK-007, CR-20 | T | planned: `link::tests::none_remedy_names_a_supported_cable` | Planned (waits for CR-20 approval) |
| AC-F01-13 | Given a Windows USB4 machine and the Linux endpoint with Thunderbolt enabled, when cabled, then Windows reports `USB4_NETWORK`, Linux reports `THUNDERBOLT`, and both prefer that link | FR-LINK-001, FR-LINK-002 | D | Manual | **CONDITIONAL (§5.2)**; interoperability unverified (CR-20) |
| AC-F01-14 | Given a Thunderbolt or USB4 controller that is present but carries a problem code, then the class is `NONE`, the driver state is `blocked`, and the explanation tells the user to ask their IT administrator | FR-LINK-005, FR-LINK-007 | T | planned: `link::tests::a_policy_blocked_controller_is_reported_as_blocked` (synthetic device list) | Planned (D-F01-2) |

## 6. Out of scope

- Choosing which interface to use → F02.
- Whether a peer is reachable over the link → F03.
- Distinguishing USB data from charge-only where the OS gives no signal (finding 1).
- USB bridge cables, USB device mode, debug links and firmware changes. CR-20
  rejects them, except a measured spike on bridge cables (D-CR20-1).

## 7. Current implementation

- [`link.rs`](../src-tauri/src/link.rs): `classify` (pure), `explain`, `observe`
  with three probes — `sysfs` (Linux), `pnp` + `setupapi` (Windows device tree),
  `iface` (macOS names). Tested against synthetic sysfs trees and device lists.
- Missing: link watcher and `link_changed` event, Windows driver state, macOS
  bridge status, measured speed in `LinkInfo`.
