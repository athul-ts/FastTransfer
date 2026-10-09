# FastTransfer
# Software Requirements Specification (SRS)

**Document Version:** 3.0
**Status:** Product Specification / Baseline
**Supersedes:** FastTransfer SRS v2.0
**Application Type:** Cross-platform desktop application
**Target Platforms:** Windows, Linux, macOS
**Frontend:** React + Vite + TypeScript
**Desktop Framework:** Tauri 2
**Native Core:** Rust
**Primary Objective:** Direct computer-to-computer file transfer over a Thunderbolt 3 / USB-C cable

---

## Revision History

| Version | Date | Summary |
|---|---|---|
| 1.0 | 2026-10-07 | Initial baseline. General-purpose transfer engine that *preferred* Thunderbolt among several co-equal transports. |
| 2.0 | 2026-10-08 | Inverted the priority: **the cable is the product**. Added §4 (connector vs. capability), §17 (cable link layer), router-free/DHCP-free discovery (FR-DISC-003/004), explicit role model (FR-SESS-002), wire framing (§18.4), pairing rate-limiting (FR-PAIR-005), chunk atomicity (FR-CHUNK-001), and Noise/TLS-1.3 crypto guidance. |
| 3.0 | 2026-10-08 | **No requirement semantics changed.** Added the formal apparatus a baseline SRS requires: RFC 2119 conformance (§114), assumptions and dependencies (§115), a risk register with the hardware premise tracked as R-01 (§116), per-requirement verification methods (§117), and a requirements traceability matrix (§118). |

---

## Conformance

The key words **MUST**, **MUST NOT**, **REQUIRED**, **SHALL**, **SHALL NOT**, **SHOULD**, **SHOULD NOT**, **RECOMMENDED**, **MAY**, and **OPTIONAL** in this document are to be interpreted as described in RFC 2119.

Throughout this document:

- **shall** / **must** — an absolute requirement. A product that does not satisfy it is non-conformant.
- **should** — a recommendation. There may be valid reasons to deviate, but the full implications must be understood and documented.
- **may** — genuinely optional.

Where a requirement uses *should* rather than *shall*, that choice is deliberate and is not an oversight. See §114 for the full conformance statement and the rationale for each *should*.

---

# 1. Document Purpose

This document defines the complete Software Requirements Specification for **FastTransfer**, a cross-platform desktop application for transferring files and folders directly between two computers.

Version 2.0 sharpened the product around a single, concrete objective, and Version 3.0 adds the formal apparatus (conformance, assumptions, risks, verification, traceability) that a baseline specification requires. The product objective itself is unchanged since v2.0:

> **Transfer files between two computers over a Thunderbolt 3 / USB-C cable, at the highest speed the hardware and cable can actually sustain.**

The v1.0 specification described a general-purpose transfer engine that *preferred* Thunderbolt among several transports. This revision inverts that priority. The **cable is the product**. Ethernet and Wi-Fi remain supported, but they exist as fallbacks for when a cable is not present — not as co-equal paths.

Everything in this document is subordinate to one question: *does this make cable transfer faster, more reliable, or easier to use?*

---

# 2. Product Vision

> Plug a cable between two computers → both see each other → pick files → transfer as fast as the link allows.
>
> Where the link is Thunderbolt, that is cable speed. Where it is not, the application says so plainly and uses the fastest path that does exist (§4.2, §5.1).

The user should never need to understand:

- IP addresses
- ports
- network interfaces
- Thunderbolt bridges
- TCP/QUIC
- routing
- link-local addressing
- filesystem protocols
- network configuration

The application should handle every one of those details automatically, and should tell the user honestly what the cable is and is not capable of.

---

# 3. Core Objective

The primary objective of FastTransfer is:

**Move data directly between two computers with no router, no switch, no Internet connection, and no cloud service involved — using the fastest direct path the two machines actually support, and reporting honestly which path that is.**

Thunderbolt networking is the **preferred** path and the product's reason to exist: when both endpoints expose a Thunderbolt-backed network interface, FastTransfer uses it and the transfer runs at cable speed. But Thunderbolt availability is a property of the hardware, not of this application, and it cannot be assumed. §4.2 states the rule; §115.1 A-01 records that it does not hold on the reference hardware; §116.1.1 records the measurement.

Success for this objective means:

1. A user connects two computers — by Thunderbolt / USB4 cable where both endpoints support it, or by Ethernet or Wi-Fi otherwise.
2. Each computer detects the other automatically, with no manual address entry.
3. The user authorizes the peer once.
4. Files transfer at the best rate the available path, the storage, and the CPU allow. Where the path is Thunderbolt the rate is limited by the cable; where it is Ethernet or Wi-Fi, by that medium. In no case is the rate limited by the application.
5. The transfer is verified, resumable, and encrypted.
6. The application reports which path it used, and never claims a capability the hardware does not have.

Everything else in this SRS supports that objective.

## 3.1 Why the objective is stated this way

An earlier revision stated the objective as cable-speed transfer over Thunderbolt, unconditionally. Measurement on the reference hardware (§116.1.1) falsified that premise: one endpoint has no Thunderbolt controller at all, and cannot acquire one.

Stating a hardware-dependent capability as the guaranteed objective would make the product's own success criteria unreachable on hardware it must nonetheless support. The objective is therefore split into what the product **guarantees** — direct, router-free, verified, encrypted transfer over the best available path — and what it **delivers when the hardware permits** — cable-speed transfer over Thunderbolt. Both are first-class. Only the second is conditional.

---

# 4. What "Thunderbolt 3 USB-C Cable" Actually Means

This distinction is foundational. A USB-C *connector* is not a capability; it is a shape. The application must never conflate the two.

## 4.1 Connector vs. Capability

| Physical link | Connector | Data capability | Usable as a transfer transport? |
|---|---|---|---|
| Thunderbolt 3 | USB-C | 40 Gbps TB + PCIe + DP | **Yes** — exposes a network interface |
| Thunderbolt 4 | USB-C | 40 Gbps TB, stricter minimums | **Yes** — exposes a network interface |
| Thunderbolt 5 | USB-C | 80/120 Gbps TB | **Yes** — exposes a network interface |
| USB4 | USB-C | 20/40 Gbps USB4 | **Sometimes** — only if TB networking is exposed |
| USB 3.2 / 3.1 / 3.0 | USB-C or A | USB data only | **No** — no network interface |
| USB 2.0 | USB-C or A | USB data only | **No** |
| Charge-only cable | USB-C | Power only | **No** |

## 4.2 The Rule

> **Thunderbolt networking is a property of the cable *and* both endpoints' controllers *and* the operating system's driver stack. Not of the connector.**

The application shall never report "Thunderbolt" solely because a USB-C connector is present. It shall report Thunderbolt only when the operating system exposes a Thunderbolt-backed network interface that can carry IP traffic.

## 4.3 Acceptable Outcomes

Given a physical cable connection, the application shall determine which of these is true and report it honestly:

```text
A. Thunderbolt networking available
   → Use it. This is the target case.

B. USB data-only cable detected
   → Report: "This cable carries data but does not provide a network
      path. Use a Thunderbolt cable, or connect over Ethernet/Wi-Fi."
   → Fall back to Ethernet / Wi-Fi if available.

C. Cable present, no data path
   → Report: "This cable appears to be charge-only."
   → Fall back to Ethernet / Wi-Fi if available.

D. No cable
   → Fall back to Ethernet / Wi-Fi.
```

Case A is the product's reason to exist. Cases B–D must degrade gracefully and explain themselves — never crash, never hang, never silently claim a speed it cannot deliver.

---

# 5. Goals

## 5.1 Primary Goals

These are **guaranteed**: they must hold on every supported platform, on any hardware the application is installed on, with no assumption about the capabilities of the network hardware. They are what v1.0 is accepted against (§99).

The system shall:

- Transfer files between two supported computers.
- Transfer complete folders.
- Automatically discover the peer on a direct link, **without requiring a router or DHCP server**.
- Establish a direct connection with no Internet dependency.
- Use the fastest path the two machines actually support, and report which one it used.
- Classify the link honestly and never claim a capability the hardware does not have (§4.2).
- Encrypt all transfers.
- Require explicit user authorization for a new peer.
- Verify transferred data with SHA-256.
- Support resumable transfers after interruption.
- Stream large files with bounded memory.
- Report **measured** throughput, not theoretical link speed.
- Support Windows, Linux, and macOS.
- Provide a polished desktop UI.
- Provide production-ready installers.

## 5.2 Conditional Goal — Thunderbolt transport

This goal is **conditional on hardware**, not on the application. It must be met when both endpoints expose a Thunderbolt-backed network interface, and cannot be met when they do not — see §3.1 and §116.1.1.

The system shall:

- Detect and use Thunderbolt networking when a Thunderbolt link is present.
- Drive that link at cable speed, limited only by the cable, storage, and CPU.
- Detect the absence of a Thunderbolt path, explain why in user terms, and fall back to §5.1's path selection without degrading the transfer's correctness guarantees.

## 5.3 Secondary Goals

The system should:

- Fall back to Ethernet, then Wi-Fi, when no cable path exists.
- Provide transfer history.
- Remember trusted devices.
- Provide diagnostics explaining why a cable path is or is not usable.
- Recover from cable unplug/replug.
- Provide actionable error messages.
- Adapt concurrency and buffering to measured conditions.

---

# 6. Non-Goals

Not part of the initial release:

- Cloud storage.
- Internet-based file transfer.
- Public file sharing.
- Peer-to-peer Internet relay.
- Operating-system migration.
- Full disk cloning.
- Remote desktop.
- Remote shell.
- Target-disk mode / block-level access.
- Automatic deletion of source files.
- Folder synchronization.

These may be reconsidered in future versions.

---

# 7. Product Scope

## 7.1 In Scope

```text
Cable / Link Detection
        ↓
Peer Discovery (link-local)
        ↓
Peer Pairing
        ↓
Transport Selection (TB → Ethernet → Wi-Fi)
        ↓
Connection Establishment
        ↓
File Selection
        ↓
Transfer Preparation
        ↓
Chunked Streaming
        ↓
Progress Monitoring
        ↓
Integrity Verification
        ↓
Transfer Completion
```

## 7.2 Supported Transfer Types

- Single file
- Multiple files
- Single folder
- Multiple folders
- Mixed files and folders

---

# 8. Target Platforms

## 8.1 Windows

Minimum:

- Windows 10 64-bit
- Windows 11 64-bit

Primary test target: Windows 11.

The application shall account for:

- Thunderbolt driver presence (Thunderbolt Control Center / Network Service)
- Windows Firewall, including per-interface rules for the Thunderbolt NIC
- Network profile classification of a direct cable link (often "Public" — blocks discovery)
- NTFS and long path handling
- UAC and non-admin operation
- Sleep/wake and dock/undock events
- USB4 / Thunderbolt device authorization prompts

## 8.2 Linux

Primary distributions:

- Ubuntu
- Debian
- Fedora
- Arch Linux

Packaging: AppImage, DEB, RPM.

The application shall account for:

- `thunderbolt` / `boltd` device authorization
- NetworkManager treating a direct link as a new connection
- mDNS/Avahi availability and systemd-resolved interaction
- Firewall (ufw/firewalld/nftables)
- Wayland and X11
- Kernel versions where Thunderbolt networking is not exposed

## 8.3 macOS

Support: Intel Macs and Apple Silicon Macs.

Packaging: DMG, signed and notarized.

The application shall account for:

- **Thunderbolt Bridge** interface (`bridge0`) creation and status
- macOS firewall and per-application network permissions
- APFS
- Apple Silicon vs. Intel architecture
- Local Network privacy permission (macOS 14+)

---

# 9. Technology Stack

```text
Frontend:      React + TypeScript + Vite
UI:            Tailwind CSS + shadcn/ui
Desktop:       Tauri 2
Native Core:   Rust
Discovery:     mDNS / DNS-SD + link-local fallback
Protocol:      TCP (v1), QUIC (evaluated for v1.0+)
Integrity:     SHA-256
Security:      Noise or TLS 1.3 via established libraries
```

The application must not implement custom cryptography.

---

# 10. High-Level Architecture

```text
┌─────────────────────────────────────────────┐
│                React UI                     │
│  Devices · Transfers · Picker · History     │
│  Settings · Diagnostics                     │
└──────────────────────┬──────────────────────┘
                       │ Tauri IPC
                       ▼
┌─────────────────────────────────────────────┐
│                 Rust Core                   │
│                                             │
│  Link Detector      (cable capability)      │
│  Discovery Manager  (peer presence)         │
│  Transport Manager  (ranking + fallback)    │
│  Connection Manager (session lifecycle)     │
│  Transfer Engine    (streaming)             │
│  Chunk Manager      (unit of resume)        │
│  Resume Manager     (state persistence)     │
│  Security Manager   (pairing + crypto)      │
│  Hash Verifier      (SHA-256)               │
│  Filesystem Manager (I/O + compat)          │
│  Diagnostics        (why isn't it working)  │
└──────────────────────┬──────────────────────┘
                       │
        ┌──────────────┼──────────────┐
        ▼              ▼              ▼
   Thunderbolt     Ethernet        Wi-Fi
    / USB4        (fallback)     (fallback)
        │              │              │
        └──────────────┼──────────────┘
                       ▼
                  Remote Device
```

---

# 11. Architectural Principles

## 11.1 UI/Core Separation

The React frontend must not implement the transfer engine. The Rust core owns networking, file I/O, hashing, encryption, protocol, resume, and connection management.

## 11.2 Transport Independence

The transfer engine must not depend directly on Thunderbolt. All transports implement one interface:

```text
Transfer Engine
       │
       ▼
Transport Interface
       │
 ┌─────┼─────┐
 ▼     ▼     ▼
TB   LAN   Wi-Fi
```

## 11.3 Cable-First, Graceful-Fallback

Thunderbolt is preferred when available. When it is not, the application degrades to Ethernet, then Wi-Fi, and always reports which path is in use.

## 11.4 Fail Gracefully

A failure in one transport must not crash the application or corrupt completed data.

## 11.5 Security by Default

Transfers are authenticated and encrypted by default. Discovery never implies authorization.

## 11.6 Streaming

Files are streamed with bounded buffers. Memory usage must not scale with file size.

## 11.7 Honest Reporting

Never display a theoretical maximum as if it were a measurement.

---

# 12. Major System Components

| Component | Responsibility |
|---|---|
| UI Layer | device display, file selection, drag/drop, progress, errors, history, settings |
| Tauri Layer | IPC, desktop integration, lifecycle, native commands |
| Rust Core | all performance-critical operations |
| Link Detector | determine cable capability; classify the physical link |
| Discovery Manager | find FastTransfer peers; link-local-first |
| Transport Manager | interface detection, ranking, selection, monitoring, fallback |
| Connection Manager | session establishment and lifecycle |
| Transfer Manager | queues, metadata, chunks, streaming, progress, retries, cancellation |
| Security Manager | identity, pairing, authentication, encryption, trust |
| Filesystem Manager | reads, writes, folders, metadata, permissions, conflicts |
| Diagnostics | explain failures in user terms |

---

# 13. Device Discovery Requirements

## FR-DISC-001
The application shall automatically discover other FastTransfer instances on directly connected and local network links.

## FR-DISC-002
Discovery shall use mDNS/DNS-SD where available.

## FR-DISC-003
**Discovery shall not depend on mDNS being available.** When mDNS is unavailable or filtered — common on a direct cable with no router — the application shall fall back to a link-local discovery mechanism (e.g., UDP broadcast on the link-local subnet, or a defined discovery port sweep on the link-local prefix).

## FR-DISC-004
**Direct-cable discovery shall work with no DHCP server present.** The application shall support IPv4 link-local (169.254.0.0/16) and IPv6 link-local (fe80::/10) addressing.

## FR-DISC-005
A device advertisement should contain: device identifier, display name, application version, operating system, protocol version, supported transport types, link capability.

## FR-DISC-006
The application shall not automatically authorize a discovered device.

## FR-DISC-007
Virtual interfaces (VPN, Docker, VirtualBox, Hyper-V, unrelated virtual adapters) shall not be selected unless explicitly configured.

## FR-DISC-008
The application shall update the device list when devices appear or disappear.

## FR-DISC-009
Discovery shall not expose user file names or private filesystem information.

## FR-DISC-010
Discovery shall be rate-limited to prevent packet floods (§91).

---

# 14. Device Identity

Every installation shall have a locally generated device identity:

```text
Device ID
Display Name
Public Identity Key
Platform
Application Version
```

The identity must remain stable across restarts. A reinstall may generate a new identity unless identity migration is implemented.

Private key material shall be generated on-device and never transmitted.

---

# 15. Pairing Requirements

## FR-PAIR-001
A user must explicitly approve a new device connection.

## FR-PAIR-002
The application shall display: device name, operating system, device identifier, connection type (and link speed when known).

## FR-PAIR-003
Pairing shall support a short verification code displayed on both endpoints, which must match before the user approves.

```text
739 284
```

## FR-PAIR-004
Verification codes shall be single-use and time-limited (recommended: 120 seconds).

## FR-PAIR-005
Pairing attempts shall be rate-limited to prevent brute force.

## FR-PAIR-006
QR-code pairing should be supported in a future release.

## FR-PAIR-007
Trusted devices may be remembered.

## FR-PAIR-008
Users shall be able to revoke a trusted device.

## FR-PAIR-009
Pairing secrets and private keys must be stored using OS-protected storage where available (Windows DPAPI/Credential Manager, macOS Keychain, Linux Secret Service).

---

# 16. Transport Requirements

Supported transports, in preference order:

```text
1. Thunderbolt / USB4 networking
2. Ethernet
3. Wi-Fi
```

## FR-TRANS-001
The application shall enumerate usable network interfaces.

## FR-TRANS-002
The application shall identify the approximate interface type.

## FR-TRANS-003
The application shall detect link capabilities where the OS exposes them.

## FR-TRANS-004
The application shall rank transports using the default preference above, adjusted by measured performance where available.

## FR-TRANS-005
Automatic selection may be overridden through advanced settings.

## FR-TRANS-006
If the preferred connection fails, the system shall attempt an available fallback when safe.

## FR-TRANS-007
Transport changes shall not corrupt completed data.

## FR-TRANS-008
The application shall always display which transport is currently in use, using measured terminology.

---

# 17. The Cable Link Layer

## FR-LINK-001
The application shall determine whether a physically connected cable exposes a usable network path.

## FR-LINK-002
The application shall classify the link into one of: `THUNDERBOLT`, `USB4_NETWORK`, `USB_DATA_ONLY`, `CHARGE_ONLY`, `NONE`.

## FR-LINK-003
The application shall not report `THUNDERBOLT` unless a Thunderbolt-backed network interface is present and can carry IP traffic.

## FR-LINK-004
On macOS, the application shall detect the Thunderbolt Bridge interface and its status.

## FR-LINK-005
On Windows, the application shall detect the presence of a Thunderbolt network adapter and its driver state.

## FR-LINK-006
On Linux, the application shall detect Thunderbolt networking support and the corresponding interface.

## FR-LINK-007
Where the link cannot be used, the application shall explain why, in user-facing language.

```text
Example:

Connection:
USB-C cable detected

Status:
This cable does not provide a network path.

FastTransfer needs a Thunderbolt cable to transfer over the cable.
Otherwise, connect both computers to the same network.

[ Use Ethernet / Wi-Fi ]
[ Retry ]
```

## FR-LINK-008
The application shall distinguish **link speed** from **measured transfer speed** at all times (§101).

## FR-LINK-009
The application shall handle cable unplug and replug without crashing, and shall attempt resume on replug where a transfer was active.

---

# 18. Network Protocol Requirements

## 18.1 Initial Protocol

The first implementation shall use TCP.

## 18.2 Advanced Protocol

QUIC should be evaluated for high-performance transfer.

## 18.3 Protocol Negotiation

Endpoints shall negotiate: protocol version, supported features, maximum chunk size, compression capability, resume capability, checksum capability, transport capabilities.

## 18.4 Wire Framing

All protocol messages shall be length-prefixed and bounded.

```text
+--------+--------+------------------+
| len:u32 (big-endian) | payload     |
+--------+--------+------------------+
```

- Maximum message size shall be defined and enforced (recommended: 16 MiB).
- Unknown message types shall be rejected, not ignored silently.
- The receiver shall treat all inbound framing as untrusted (§90).

---

# 19. Session Establishment

```text
CLIENT                          SERVER
  │                                │
  │──────── HELLO ────────────────▶│
  │◀─────── HELLO_ACK ─────────────│
  │──────── AUTHENTICATE ─────────▶│
  │◀─────── AUTH_OK ───────────────│
  │──────── TRANSFER_REQUEST ─────▶│
  │◀─────── ACCEPT / REJECT ───────│
  │◀════════ TRANSFER ════════════▶│
```

## FR-SESS-001
The wire protocol must be versioned.

## FR-SESS-002
The role model shall be explicit. The peer that initiates the pairing/connection request becomes the **sender**; the accepting peer becomes the **receiver**. Simultaneous initiation shall be resolved deterministically (e.g., lowest device ID becomes receiver).

## FR-SESS-003
Incompatible protocol versions shall be rejected safely, with a clear message on both sides.

## FR-SESS-004
Authentication must complete before any file data is transmitted.

---

# 20. Transfer Requirements

## FR-TRANSFER-001
The user shall be able to select one or more files.

## FR-TRANSFER-002
The user shall be able to select folders.

## FR-TRANSFER-003
The user shall be able to drag and drop files into the application.

## FR-TRANSFER-004
The application shall preserve relative directory structure.

## FR-TRANSFER-005
The application shall stream file contents.

## FR-TRANSFER-006
The application shall support large files (100 GB+ tested).

## FR-TRANSFER-007
The application shall display transfer progress.

## FR-TRANSFER-008
The user shall be able to cancel an active transfer.

## FR-TRANSFER-009
The user should be able to pause and resume transfers.

## FR-TRANSFER-010
The application shall support multiple queued transfers.

---

# 21. File Chunking

Files shall be divided into logical chunks:

```text
File
├── Chunk 000
├── Chunk 001
├── ...
└── Chunk N
```

## FR-CHUNK-001
**A chunk is the atomic unit of resume.** Partial chunks must never be persisted as complete.

## FR-CHUNK-002
Chunk size shall be configurable internally and optimized for the active transport.

## FR-CHUNK-003
Chunk size shall be negotiated between endpoints at session start.

## FR-CHUNK-004
Chunk metadata shall not create excessive filesystem overhead.

---

# 22. Resume Requirements

## FR-RESUME-001
An interrupted transfer shall be resumable.

## FR-RESUME-002
The receiver shall be able to report completed chunks.

## FR-RESUME-003
The sender shall transfer only missing or invalid chunks.

## FR-RESUME-004
Resume metadata shall survive application restart where practical.

## FR-RESUME-005
A partial transfer shall never be presented as a complete file.

```text
Transfer interrupted

92.4 GB / 100 GB

[ Resume ]
[ Cancel ]
```

## FR-RESUME-006
On cable replug, an active transfer may resume automatically according to user settings.

---

# 23. Integrity Verification

The system shall compute cryptographic hashes for transferred data. SHA-256 shall be supported.

```text
Source:       8f72...a91c
Destination:  8f72...a91c

✓ Verified
```

Verification shall occur after transfer. For large files, hashing shall be streamed. Verification failures shall be reported clearly and shall mark the transfer as failed, not complete.

---

# 24. File Conflict Handling

If a destination file exists, offer:

```text
Replace
Skip
Keep Both
Cancel
```

For multiple files, offer **Apply to all**.

**Default: Keep Both.** Destructive overwrite shall never be the default and shall require explicit confirmation.

---

# 25. Filesystem Requirements

The application shall support regular files and directories, and should preserve relative paths, modification timestamps, permissions where compatible, and symbolic links where safely supported.

## FR-FS-001
Unsupported metadata shall not fail the entire transfer unless essential.

## FR-FS-002
**Symbolic links on Windows:** where symlink creation is unavailable (non-admin, no Developer Mode), the application shall not silently fail. It shall record the link target in a sidecar manifest or skip the link with a clear warning.

## FR-FS-003
The application shall account for NTFS, APFS, ext4, btrfs, and other supported filesystems.

---

# 26. Filename Compatibility

The system shall detect incompatible filenames, including Windows reserved characters and device names, path length limits, case sensitivity differences, and Unicode normalization differences.

```text
Filename cannot be created on Windows.

Original:
project:/test?.txt

[ Rename ]
[ Skip ]
[ Cancel ]
```

## FR-NAME-001
Unicode normalization differences (NFC vs. NFD, macOS vs. Linux) shall not cause duplicate or missing files.

## FR-NAME-002
The application shall provide a safe, non-destructive resolution mechanism.

---

# 27. Transfer Queue

```text
Transfer Queue

1. project.zip      4.2 GB    Sending...
2. Videos/         18.2 GB    Waiting
3. Photos/          6.8 GB    Waiting
```

Users may reorder pending transfers, cancel pending or active transfers, and retry failed transfers.

---

# 28. Parallel Transfer

The transfer engine may use parallelism, adapting to file size, storage speed, CPU, network throughput, latency, and available memory.

## FR-PAR-001
The system shall enforce a hard cap on simultaneous streams (recommended: 4 per transfer, 16 total) to prevent resource exhaustion.

---

# 29. Backpressure

```text
Fast Network
     ↓
Transfer Buffer
     ↓
Slow Destination SSD
```

If the receiver cannot consume data fast enough, the sender must slow down. This prevents uncontrolled RAM growth, queue explosion, and instability.

---

# 30. Performance Requirements

## NFR-PERF-001
The application shall use available bandwidth efficiently.

## NFR-PERF-002
The application shall avoid unnecessary memory copies.

## NFR-PERF-003
The application shall stream large files.

## NFR-PERF-004
The application shall report actual **measured** throughput.

## NFR-PERF-005
The application should support multi-GB/s throughput on hardware where the connection and storage support it.

## NFR-PERF-006
The application shall not impose a fixed low speed ceiling.

## NFR-PERF-007
On a Thunderbolt link, the application shall saturate the link up to the limits of source and destination storage.

---

# 31. Example Performance Targets

Targets, not guarantees. Validated on representative hardware.

```text
Thunderbolt 3/4 (40 Gbps link)
  Target: several GB/s where storage permits

Thunderbolt 5 (80/120 Gbps link)
  Target: several GB/s where the full system permits

10 Gb Ethernet
  Target: ~1 GB/s

2.5 Gb Ethernet
  Target: ~250+ MB/s

1 Gb Ethernet
  Target: ~100+ MB/s

Wi-Fi (typical)
  Target: measured, reported honestly
```

Actual performance shall be reported from measurement, never from the link rating.

---

# 32. Memory Requirements

Memory usage must not scale with file size. A 100 GB file must not require 100 GB RAM.

The transfer engine shall use bounded buffers, predictable memory usage, configurable buffer pools, and streaming I/O.

---

# 33. CPU Requirements

The transfer engine should minimize unnecessary CPU usage. Compression shall not be enabled blindly for already-compressed data (ZIP, MP4, MKV, JPEG, PNG, ISO).

---

# 34. Security Requirements

## NFR-SEC-001
All authenticated transfers shall be encrypted.

## NFR-SEC-002
Discovered devices shall not automatically gain filesystem access.

## NFR-SEC-003
Users shall explicitly authorize new devices.

## NFR-SEC-004
Cryptographic libraries shall be established and maintained.

## NFR-SEC-005
Private keys shall be protected using OS secure storage where possible.

## NFR-SEC-006
File data shall not be transmitted before authorization.

## NFR-SEC-007
Logs shall not contain file contents.

## NFR-SEC-008
Credentials and keys shall not be written to plain-text logs.

## NFR-SEC-009
**The session handshake shall use a documented pattern from an established library.** Recommended: the Noise Protocol Framework (`Noise_XX_25519_ChaChaPoly_BLAKE2s`) for pairing, or TLS 1.3. Custom handshake construction is prohibited.

---

# 35. Authentication

The application shall authenticate the remote device before transferring files, combining device identity, secure session establishment, explicit user approval, and pairing verification.

---

# 36. Trusted Devices

```text
Athul-PC

✓ Trusted

[ Revoke Trust ]
```

Trusted devices may reconnect without full manual approval, subject to the security model.

---

# 37. Firewall Handling

The application shall detect likely firewall-related connectivity problems and explain them.

```text
Unable to connect.

Windows Firewall may be blocking FastTransfer on the
Thunderbolt connection.

[ Open Network Settings ]
[ Retry ]
[ Cancel ]
```

The application must not silently disable the firewall.

## FR-FW-001
On Windows, the application shall detect when a direct cable link is classified as a "Public" network profile, which commonly blocks discovery, and shall explain this specifically.

---

# 38. User Interface Requirements

## Main Screen

```text
┌────────────────────────────────────────────┐
│ FastTransfer                         ⚙    │
├────────────────────────────────────────────┤
│  Link: ⚡ Thunderbolt 4 · 40 Gbps          │
│        Measured: 2.7 GB/s                  │
├────────────────────────────────────────────┤
│ Nearby Devices                             │
│                                            │
│ ⚡ Athul-MacBook                           │
│    macOS · Thunderbolt                     │
│                                            │
│ 🖥 Athul-PC                                │
│    Windows 11 · Ethernet                   │
├────────────────────────────────────────────┤
│                                            │
│          Drop files here                   │
│                                            │
│              or                            │
│                                            │
│         [ Select Files ]                   │
└────────────────────────────────────────────┘
```

---

# 39. Device Card

```text
⚡ Athul-PC

Windows 11
Thunderbolt 4
Link: 40 Gbps

Trusted

[ Send Files ]
```

Displays: device name, OS, connection type, connection quality, trusted state, availability.

---

# 40. Transfer Screen

```text
Sending

project.zip

████████████████████░░ 82%

3.8 GB / 4.6 GB

Speed:
2.18 GB/s

Time remaining:
00:00:04

Connection:
Thunderbolt

Destination:
Athul-MacBook

[ Pause ]     [ Cancel ]
```

---

# 41. Transfer Completion

```text
Transfer Complete

project.zip

4.6 GB transferred
2.31 GB/s average

Integrity:
✓ Verified

Destination:
Athul-MacBook

[ Open Folder ]
[ Done ]
```

---

# 42. Error UI

Bad:

```text
ECONNRESET
```

Good:

```text
Connection interrupted.

The cable was disconnected or the destination is no longer reachable.

Your transfer can be resumed.

[ Resume ]
[ Cancel ]
```

---

# 43. Diagnostics

```text
Device:       Athul-PC
OS:           Windows 11

Interfaces:

  Thunderbolt    40 Gbps    Available
  Ethernet      2.5 Gbps    Available
  Wi-Fi                     Available

Cable:

  Detected:    Yes
  Classified:  THUNDERBOLT
  Bridge:      Up

Firewall:      Possibly blocking inbound connection
Discovery:     Working (mDNS)
Pairing:       Trusted
```

---

# 44. Settings

**General:** device name, startup behavior, notifications.

**Transfers:** default download location, overwrite behavior, concurrency, verification, resume behavior.

**Network:** automatic transport selection, preferred transport, allowed interfaces, discovery settings.

**Security:** trusted devices, pairing, session security settings.

**Advanced:** diagnostics, logging level, developer mode.

---

# 45. Default Download Location

```text
Windows:  Downloads\FastTransfer
Linux:    ~/Downloads/FastTransfer
macOS:    ~/Downloads/FastTransfer
```

The user may select another destination.

---

# 46. Transfer History

```text
Today

✓ project.zip     4.2 GB   Athul-MacBook   12:42 PM
✓ Photos/        18.7 GB   Windows-PC      11:31 AM
✕ Backup.iso      Interrupted                9:15 AM
```

History shall contain metadata only and shall not store file contents.

---

# 47. Notifications

Desktop notifications for: transfer completed, transfer failed, connection lost, transfer resumed, device connected (cable plugged in), incoming transfer request. Users may disable notifications.

---

# 48. Incoming Transfer

```text
Incoming Transfer

From:         Athul-PC
Files:        project.zip, videos/
Total:        22.4 GB
Destination:  Downloads/FastTransfer

[ Accept ]
[ Reject ]
```

The receiver must be able to reject.

---

# 49. Destination Safety

The application shall prevent accidental writes to dangerous locations:

```text
C:\Windows
C:\Program Files
/usr/bin
/System
```

unless explicitly authorized and technically appropriate.

---

# 50. File Permissions

The application shall not silently elevate privileges. If a destination requires administrative permission, it shall inform the user and offer alternatives.

---

# 51. Logging

Structured logging with levels `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`. Production default: `INFO`. Sensitive data and file contents must never be logged. Log rotation and a size cap shall be enforced.

---

# 52. Crash Handling

The application should recover gracefully after network interruption, filesystem failure, remote crash, local restart, cable unplug, and system sleep/wake. A crash must never silently mark an incomplete transfer as successful.

---

# 53. Application Restart

After restart, restore pending transfers where safe, resumable transfer metadata, trusted devices, and user settings. Incomplete transfers shall be shown clearly.

---

# 54. Sleep and Wake

```text
On sleep:   Transfer: Paused
After wake: Connection: Reconnecting...
            Transfer: Resuming...
```

---

# 55. Connection Loss

```text
Connection lost

Transferred:  72.4 GB / 100 GB
Status:       Waiting for connection...

[ Retry ]
[ Cancel ]
```

If the remote becomes available again, the transfer may resume automatically per user settings.

---

# 56. Multiple Device Support

The application may display multiple nearby devices. Only authorized transfers may occur. With a cable, typically exactly one peer is expected.

---

# 57. Multi-Device Transfers

Initial release: one active destination per transfer.
Future release: simultaneous transfers to multiple devices.

---

# 58. Protocol Versioning

The protocol must contain an explicit version. Incompatible versions shall be rejected safely. Backward compatibility should be maintained where practical.

---

# 59. Capability Negotiation

Endpoints should negotiate protocol version, maximum chunk size, resume support, hash support, transport features, compression support, and encryption capability.

```text
Client: Protocol 1.2
Server: Protocol 1.1
Negotiated: Protocol 1.1
```

---

# 60. Compression

Compression shall be optional. The application should detect likely incompressible files (JPEG, PNG, MP4, MKV, ZIP, 7z, RAR, ISO) and skip compression for them. Compression may be beneficial for text-heavy data on slower links.

---

# 61. Adaptive Transfer

The transfer engine should measure throughput, latency, packet loss where applicable, disk performance, and CPU utilization, and may adjust chunk size, stream count, buffering, and concurrency.

---

# 62. Benchmarking

A dedicated benchmark mode should be considered, clearly labelled as a benchmark, and must never overwrite user files.

```text
Connection Benchmark

Thunderbolt 4
Link:   40 Gbps

Write:  2.74 GB/s
Read:   2.91 GB/s

Network: Excellent
```

---

# 63. Performance Testing

Test sizes: 1 GB, 10 GB, 50 GB, 100 GB, 500 GB, 1 TB+.

Test types: large ISO, large ZIP, video, many small files, mixed directories.

---

# 64. Small-File Optimization

A directory of thousands of small files performs poorly if every file requires an expensive independent operation. The application shall optimize metadata batching, directory creation, hashing, scheduling, and filesystem operations. 100,000 small files must be handled differently from one 100 GB file.

---

# 65. Transfer Estimation

Estimated remaining time shall be computed from a moving throughput window, not instantaneous speed. The UI shall avoid unstable estimates.

---

# 66. Accessibility

## NFR-A11Y-001
The UI shall target **WCAG 2.1 Level AA**.

Requirements: keyboard navigation, visible focus states, readable contrast, screen-reader-friendly labels, scalable UI where practical, accessible controls.

---

# 67. Internationalization

The architecture shall allow future localization. Initial language: English. Future: Malayalam, Hindi, others. All user-facing strings shall be externalized.

---

# 68. Theme Support

```text
Appearance

○ Light
○ Dark
● System
```

---

# 69. Application Startup

Startup should be fast: initialize UI, initialize Rust core, initialize discovery, enumerate interfaces, show available devices. The UI shall not block while discovery runs.

---

# 70. Resource Limits

Controlled limits for concurrent transfers, streams, buffer memory, discovery frequency, and log size, to prevent resource exhaustion.

---

# 71. Privacy Requirements

The application shall not upload file contents to a cloud service, not require an account for local transfers, not collect unnecessary file metadata, not transmit file names outside the participating devices, and not include file contents in telemetry. Any optional telemetry must be disclosed and configurable.

---

# 72. Offline Operation

Core transfer functionality must work without Internet access. Internet access shall not be required for discovery, pairing, transfer, or verification.

---

# 73. Internet Independence

This scenario must work:

```text
Laptop A
   │
   │ Thunderbolt 3 cable
   │
Laptop B

No Internet. No router. No switch.
```

This is the **primary use case**, not an edge case. It shall be covered by automated and manual testing.

---

# 74. Data Model

Core entities:

```text
Device
TrustedDevice
Transport
LinkInfo
Connection
Transfer
TransferItem
Chunk
TransferSession
TransferHistory
ApplicationSettings
```

---

# 75. Device Model

```text
deviceId
displayName
platform
platformVersion
appVersion
protocolVersion
identityKey
trusted
lastSeen
availableTransports
linkCapability
```

---

# 76. Transfer Model

```text
transferId
sourceDevice
destinationDevice
status
createdAt
startedAt
completedAt
totalBytes
transferredBytes
averageSpeed
currentSpeed
transport
items
verificationStatus
error
```

---

# 77. Transfer Statuses

```text
QUEUED
PREPARING
CONNECTING
AUTHENTICATING
TRANSFERRING
VERIFYING
COMPLETED
PAUSED
INTERRUPTED
FAILED
CANCELLED
```

---

# 78. Transfer Item Statuses

```text
PENDING
TRANSFERRING
VERIFYING
COMPLETED
SKIPPED
FAILED
```

---

# 79. API / IPC Design

The React UI shall communicate with the Rust core through Tauri commands and events.

**Commands:**

```text
get_devices
get_transports
get_link_info
pair_device
remove_trusted_device
select_files
start_transfer
pause_transfer
resume_transfer
cancel_transfer
retry_transfer
get_transfer
get_transfer_history
get_diagnostics
get_settings
update_settings
```

**Events:**

```text
device_discovered
device_updated
device_removed

link_changed

transfer_started
transfer_progress
transfer_paused
transfer_resumed
transfer_completed
transfer_failed
transfer_cancelled

connection_changed
diagnostic_update
```

---

# 80. UI State Management

The React application should maintain state for devices, selectedDevice, transfers, activeTransfer, connectionStatus, notifications, settings, history, and diagnostics.

The frontend shall not duplicate authoritative transfer state. **Rust remains the source of truth.**

---

# 81. Tauri Security

Tauri capabilities and permissions shall be minimized. Only required native functionality shall be exposed to the frontend. Unrestricted filesystem access shall not be exposed to the UI layer.

---

# 82. Auto Update

Future production releases should support updates that verify update signatures, avoid installing untrusted binaries, provide release version information, and allow user-controlled update settings.

---

# 83. Installer Requirements

**Windows:** MSI and EXE installer; Start Menu entry; optional desktop shortcut; uninstall registration; upgrade handling.

**Linux:** AppImage, DEB, RPM.

**macOS:** DMG with signing and notarization for production.

---

# 84. Code Signing

**Windows:** code-signing certificate.
**macOS:** Apple Developer signing and notarization.
**Linux:** package signing where appropriate.

---

# 85. CI/CD

CI shall build Windows, Linux, macOS Intel, and macOS Apple Silicon.

```text
Commit → Lint → Type Check → Unit Tests → Rust Tests
       → Integration Tests → Build → Package
       → Security Checks → Release
```

---

# 86. Testing Strategy

**Unit:** chunking, hashing, metadata, state machine, path handling, protocol encoding/decoding.

**Integration:** discovery, pairing, connection, transfer, resume, verification.

**End-to-End:** complete transfers across platforms, including over a real cable.

---

# 87. Cross-Platform Test Matrix

| Source | Destination |
|---|---|
| Windows | Windows |
| Windows | Linux |
| Windows | macOS |
| Linux | Windows |
| Linux | Linux |
| Linux | macOS |
| macOS | Windows |
| macOS | Linux |
| macOS | macOS |

---

# 88. Transport Test Matrix

For each of Thunderbolt/USB4, Ethernet, and Wi-Fi, test: discovery, pairing, transfer, interruption, resume, verification, fallback.

**Additionally, for the cable path:**

- Thunderbolt cable with no router present
- Thunderbolt cable with no DHCP server present
- Thunderbolt cable with no Internet
- USB data-only cable (must classify correctly and fall back)
- Charge-only cable (must classify correctly and fall back)
- Cable unplug mid-transfer, then replug
- Peer sleeps and wakes while cabled

---

# 89. Failure Testing

Test: cable unplugged, cable replugged, Wi-Fi disabled, Ethernet disconnected, remote application closed, computer sleeps, disk full, destination permission denied, source file disappears, destination file changes, application crashes, system restarts, firewall blocks connection.

---

# 90. Security Testing

Test: unauthorized device, invalid authentication, replay attempts, invalid transfer requests, malformed packets, protocol version mismatch, invalid metadata, path traversal, malicious filenames, oversized metadata, resource exhaustion.

---

# 91. Path Traversal Protection

Incoming file paths must never be allowed to escape the selected destination directory.

```text
../../important-file
```

must be rejected or safely normalized. **This is a critical security requirement.**

---

# 92. Malicious Input Handling

The receiver must treat all remote metadata as untrusted. Validate filename, path, file size, chunk index, chunk size, hash, transfer ID, and protocol version.

Never trust a remote device merely because it is physically cabled or discovered on the LAN.

---

# 93. Denial-of-Service Protection

The application should protect against excessive discovery packets, enormous metadata requests, huge transfer queues, excessive streams, malformed protocol messages, and uncontrolled memory allocation.

---

# 94. Backup of Transfer State

Transfer state should be stored safely enough to permit recovery after application restart, and shall not contain unnecessary sensitive information.

---

# 95. Accessibility of Errors

Every error shall state:

```text
What happened
Why it happened, if known
What the user can do
```

```text
Transfer failed

The destination disk is full.

Free space:   2.1 GB
Required:    14.7 GB

[ Choose Another Folder ]
[ Retry ]
```

---

# 96. Functional Requirements Summary

| ID | Requirement | Priority |
|---|---|---|
| FR-LINK-001 | Cable capability detection | Critical |
| FR-LINK-003 | No false Thunderbolt claims | Critical |
| FR-DISC-004 | Router-free discovery | Critical |
| FR-DISC-003 | mDNS-independent discovery | Critical |
| FR-PAIR-001 | Device authorization | Critical |
| FR-TRANS-001 | Interface detection | Critical |
| FR-TRANSFER-001 | File transfer | Critical |
| FR-TRANSFER-002 | Folder transfer | Critical |
| FR-TRANSFER-005 | Streaming | Critical |
| FR-TRANSFER-006 | Large files | Critical |
| FR-RESUME-001 | Resume | Critical |
| FR-CHUNK-001 | Chunk atomicity | Critical |
| FR-TRANSFER-007 | Progress | High |
| FR-TRANSFER-008 | Cancellation | High |
| FR-TRANSFER-010 | Queue | High |
| FR-TRANS-006 | Fallback | High |
| FR-PAIR-008 | Revoke trusted devices | High |
| FR-TRANSFER-004 | Directory preservation | High |
| FR-TRANSFER-003 | Drag/drop | High |
| FR-TRANSFER-009 | Pause/resume | Medium |

This table lists the critical and high-priority requirements only. For their owning
development phase, verification method, and current verification status, see the
traceability matrix in **§118**.

---

# 97. Non-Functional Requirements Summary

| Category | Requirement |
|---|---|
| Performance | Saturate the cable link up to storage limits |
| Reliability | Resume interrupted transfers |
| Security | Encrypted, authenticated, no custom crypto |
| Privacy | No cloud dependency, no account |
| Compatibility | Windows / Linux / macOS |
| Scalability | Multi-GB/TB files, bounded memory |
| Usability | No networking knowledge required |
| Maintainability | Rust core separated from UI |
| Accessibility | WCAG 2.1 AA |
| Observability | Structured diagnostics and logging |
| Recovery | Handle unplug, sleep, crash |
| Packaging | Native signed installers |

How each non-functional requirement is verified — by test, demonstration, analysis,
or inspection — is defined in **§117**.

---

# 98. MVP Definition

The first MVP shall contain:

```text
✓ React + Vite + TypeScript
✓ Tauri 2 + Rust core
✓ Cable/link classification
✓ Router-free peer discovery
✓ Device pairing
✓ TCP transport
✓ File transfer
✓ Folder transfer
✓ Progress
✓ Cancellation
✓ SHA-256 verification
✓ Basic resume
✓ Windows
✓ Linux
✓ macOS
```

The MVP does not need:

```text
✗ QUIC
✗ Multi-device broadcast
✗ Clipboard sharing
✗ Computer migration
✗ Benchmarking suite
✗ Cloud integration
```

---

# 99. Version 1.0 Definition

```text
✓ Cross-platform support
✓ Cable link classification and honest reporting
✓ Automatic discovery (mDNS + link-local fallback)
✓ Secure pairing (Noise or TLS 1.3)
✓ Ethernet and Wi-Fi fallback
✓ Automatic transport selection
✓ TCP; QUIC evaluated
✓ Large files and folder transfer
✓ Resume
✓ Hash verification
✓ Conflict handling
✓ Transfer queue
✓ Transfer history
✓ Diagnostics
✓ Light / dark / system theme
✓ Native packaging and signing

~ Thunderbolt / USB4 transport          conditional on hardware (§5.2)
```

Every item marked `✓` is guaranteed by §5.1 and must hold on any hardware the application runs on. The single item marked `~` is a §5.2 conditional goal: it must work when both endpoints expose a Thunderbolt-backed network interface, and v1.0 does **not** require it to be demonstrated on any particular machine. A build that correctly detects the absence of a Thunderbolt path, explains it, and transfers over the best available path meets v1.0.

This distinction matters because the reference hardware cannot exhibit the `~` case at all (§116.1.1); an acceptance bar keyed to it would be permanently unreachable.

---

# 100. Future Version Features

**v1.1** — QR pairing, richer diagnostics, performance tuning.
**v1.2** — clipboard sharing, small-file optimization, richer history.
**v2.0** — multi-device transfers, computer migration, selective sync, remote folder browsing.

---

# 101. Product Performance Philosophy

The application shall never advertise a transfer speed derived from the link rating.

```text
Wrong:

"40 Gbps transfer"
```

```text
Right:

Connection:  Thunderbolt 4
Link:        40 Gbps
Current:     2.37 GB/s  (measured)
```

This distinction is a core product principle.

---

# 102. Example Complete User Flow

**Sender**

```text
Open FastTransfer
      ↓
Plug Thunderbolt cable
      ↓
Cable classified: THUNDERBOLT
      ↓
Peer discovered automatically (no router)
      ↓
Peer requests authorization
      ↓
User accepts (verification code matches)
      ↓
Select project folder
      ↓
Select destination
      ↓
FastTransfer selects Thunderbolt
      ↓
Transfer starts
      ↓
Progress displayed (measured speed)
      ↓
Transfer completed
      ↓
SHA-256 verified
```

**Receiver**

```text
Open FastTransfer
      ↓
Cable plugged in
      ↓
Peer discovered
      ↓
Receive request
      ↓
Review sender and files
      ↓
Accept
      ↓
Choose destination
      ↓
Receive chunks
      ↓
Verify
      ↓
Complete
```

---

# 103. Example Interrupted Transfer

```text
Transfer:     100 GB
Transferred:  67.4 GB

Cable unplugged
      ↓
Transfer marked INTERRUPTED
      ↓
Application waits
      ↓
Cable replugged
      ↓
Session re-established
      ↓
Receiver reports completed chunks
      ↓
Only missing chunks transferred
      ↓
100 GB completed
      ↓
SHA-256 verification
```

---

# 104. Example Connection Selection

```text
Available:

  Thunderbolt   40 Gbps link    Measured 2.7 GB/s
  Ethernet     2.5 Gbps link    Measured 270 MB/s
  Wi-Fi                         Measured 610 MB/s

Link classification:

  THUNDERBOLT

Selected: Thunderbolt
```

---

# 105. Example Failure

```text
Destination disk full

Required:   80 GB
Available:  12 GB

Transfer paused.

[ Choose Another Location ]
[ Cancel ]
```

Already-transferred data should be preserved where safe resume is possible.

---

# 106. Acceptance Criteria

## Cable Link

- A Thunderbolt 3 cable between two machines **that both expose a Thunderbolt-backed network interface** is classified as `THUNDERBOLT`.
- A cable between two machines where at least one exposes no such interface is **never** classified as `THUNDERBOLT`. The application reports the best path that does exist, or none (§116.1.1).
- A USB data-only cable is classified as `USB_DATA_ONLY` and does not claim Thunderbolt.
- A charge-only cable is classified as `CHARGE_ONLY`.
- In every case, the application explains the result in user terms.

## Connectivity

- Windows ↔ Windows, Windows ↔ Linux, Windows ↔ macOS
- Linux ↔ Linux, Linux ↔ macOS
- macOS ↔ macOS
all work **with no router, no DHCP, and no Internet** — over a direct cable link where the hardware provides one, and over direct Ethernet or Wi-Fi where it does not. The router-free requirement applies to the direct-cable case; the no-Internet and no-cloud requirements apply to every case.

## Transfer

- files, folders, large files, and small-file collections transfer correctly
- progress is accurate and reflects measured speed
- cancellation works
- resume works, including across cable unplug/replug
- verification works and failures are reported as failures

## Security

- unauthorized devices cannot transfer
- transfers are encrypted
- malicious paths are rejected
- malformed messages do not crash the application

## Reliability

- cable unplug is recovered
- application restart recovers resumable transfers
- sleep/wake is handled
- disk-full and permission errors are understandable

## Performance

- no artificial throughput ceiling
- memory remains bounded with 100 GB+ files
- multi-GB/s is achievable on capable hardware
- statistics reflect measurement

## Packaging

- Windows installer works
- Linux packages work
- macOS package works
- production builds are signed where applicable

---

# 107. Definition of Done

```text
Implementation
    ↓
Unit Tests
    ↓
Integration Tests
    ↓
Cross-Platform Tests
    ↓
Cable-Path Tests
    ↓
Error Handling
    ↓
Security Review
    ↓
Performance Validation
    ↓
UI Validation
    ↓
Documentation
    ↓
Done
```

---

# 108. Recommended Development Phases

## Phase 1 — Foundation
React, Vite, TypeScript, Tauri, Rust, CI.
**Deliverable:** cross-platform application opens.

## Phase 2 — Cable Link Detection
Link classification, interface enumeration, Thunderbolt bridge detection.
**Deliverable:** application correctly reports what the plugged cable can do.

## Phase 3 — Router-Free Discovery
Link-local addressing, discovery fallback, device identity.
**Deliverable:** two cabled computers see each other with no router.

## Phase 4 — Basic Transfer
TCP, file transfer, progress, verification.
**Deliverable:** 1 GB file transfers over the cable.

## Phase 5 — Folder Transfer
Directory traversal, metadata, filesystem compatibility, conflict handling.

## Phase 6 — Reliability
Chunking, resume, retry, cancellation, recovery, replug handling.

## Phase 7 — Security
Pairing, authentication, encryption, trusted devices.

## Phase 8 — Performance
Parallel streams, adaptive buffering, backpressure, transport selection, QUIC evaluation.

## Phase 9 — Production UI
Drag/drop, history, settings, diagnostics, notifications.

## Phase 10 — Release
Windows, Linux, macOS, signing, packaging, auto-update.

---

# 109. Final Architecture

```text
                         FASTTRANSFER
                              │
          ┌───────────────────┴───────────────────┐
          │                                       │
          ▼                                       ▼
   React + Vite                              Rust Core
   TypeScript                                    │
   Tailwind                                     │
   shadcn/ui                                    │
          │                        ┌─────────────┼─────────────┐
          ▼                        │             │             │
       Tauri 2              Link Detector   Discovery      Security
          │                        │             │             │
          └────────────────────────┼─────────────┼─────────────┘
                                   │             │
                                   ▼             ▼
                            Transport Manager / Transfer Engine
                                   │
                      ┌────────────┼────────────┐
                      ▼            ▼            ▼
                 Thunderbolt    Ethernet      Wi-Fi
                   / USB4      (fallback)   (fallback)
                      │            │            │
                      └────────────┼────────────┘
                                   ▼
                             Remote Device
```

---

# 110. Final Technology Decision

```text
Frontend:            React + TypeScript + Vite
UI:                  Tailwind CSS + shadcn/ui
Desktop:             Tauri 2
Native Core:         Rust
Discovery:           mDNS / DNS-SD + link-local fallback
Initial Protocol:    TCP
High-Performance:    QUIC (evaluated)
Integrity:           SHA-256
Security:            Noise or TLS 1.3 via established libraries
Primary Transport:   Thunderbolt 3 / USB4
Fallback 1:          Ethernet
Fallback 2:          Wi-Fi
Platforms:           Windows, Linux, macOS
```

---

# 111. Core Product Principle

> **FastTransfer is a cable-first direct-transfer engine. It uses the fastest safe connection available — preferring Thunderbolt, falling back to Ethernet and Wi-Fi — and it always reports what it actually measured, never what the connector promised.**

The user should see:

```text
Connect → Discover → Select → Send → Verify → Done
```

while the application handles transport detection, network configuration, authentication, encryption, chunking, streaming, parallelism, resume, hashing, filesystem compatibility, and error recovery behind the scenes.

---

# 112. First Engineering Milestone

Before any UI work, the first technical milestone shall be a Rust proof of concept:

```text
Laptop A
   │
   │ Thunderbolt 3 cable
   │
Laptop B

Rust Proof of Concept

1.  Detect the cable and classify the link
2.  Enumerate interfaces; identify the Thunderbolt path
3.  Discover the peer with no router and no DHCP
4.  Establish a connection
5.  Authenticate (pairing)
6.  Transfer 1 GB
7.  Measure actual throughput
8.  Compute SHA-256 on both ends
9.  Verify the destination
10. Disconnect cleanly
```

This must then be tested on:

```text
Windows ↔ Linux
Windows ↔ macOS
Linux ↔ macOS
```

And repeated with:

```text
- a USB data-only cable  (must classify correctly)
- no router / no DHCP    (must still discover)
```

Only after this milestone succeeds shall the production transfer engine and React/Tauri UI be built on top of the validated networking architecture.

---

# 113. SRS Baseline Summary

FastTransfer is defined as:

> **A secure, cross-platform, cable-first, high-performance direct computer-to-computer file-transfer application built with React + Vite + Tauri 2 + Rust, whose primary objective is fast, verified, resumable data transfer over a Thunderbolt 3 / USB-C cable with no router, no DHCP, and no Internet dependency.**

Its primary differentiators:

- cable-first transport detection with honest classification
- router-free, DHCP-free direct-link discovery
- Thunderbolt / USB4 transfer at cable speed
- Ethernet and Wi-Fi fallback
- secure pairing and encrypted sessions
- resumable transfers across cable unplug/replug
- SHA-256 integrity verification
- cross-platform support with native installers
- measured, not advertised, performance

The architecture deliberately separates the UI from the native transfer engine so the networking and storage layers can be optimized independently from the user interface.

This SRS is the baseline specification for implementation and shall be updated as hardware testing, protocol experiments, security review, and UX validation produce concrete engineering decisions.

---

# 114. Document Conformance

## 114.1 Requirement Language

Requirement keywords are interpreted per RFC 2119 (see the Conformance statement in the front matter):

| Keyword | Meaning | Binding? |
|---|---|---|
| shall / must | Absolute requirement | Yes — non-conformance blocks release |
| shall not / must not | Absolute prohibition | Yes |
| should | Recommended; deviation requires documented justification | Soft |
| may | Optional | No |

Every requirement in this document carries an identifier of the form `FR-<AREA>-<NNN>` (functional) or `NFR-<AREA>-<NNN>` (non-functional). Identifiers are stable and shall not be renumbered once assigned; withdrawn requirements are marked withdrawn rather than reused.

## 114.2 Where *should* Is Used, and Why

The following requirements use *should* deliberately. Each is a case where hardware, OS, or vendor behaviour is outside the application's control, so an absolute requirement would be unverifiable or dishonest.

| Requirement | Why it is *should*, not *shall* |
|---|---|
| FR-DISC-005 | Advertisement contents depend on what the OS exposes; the app can only advertise what it knows. |
| FR-PAIR-006 | QR pairing is a convenience over the required verification-code flow. |
| FR-PAIR-007 | Remembering trusted devices is a usability choice with a security trade-off the user owns. |
| FR-TRANS-005 | Overriding automatic transport selection is an advanced-user affordance. |
| FR-TRANS-006 | Fallback depends on an alternative path existing. |
| FR-TRANSFER-009 | Pause/resume is only meaningful where the transport supports it safely. |
| FR-LINK-* (probe depth) | The depth of link information available is OS-dependent. |
| NFR-PERF-005 | Multi-GB/s is not achievable on all hardware; it is a target, not a guarantee. |

No *should* in this document may be silently dropped. Each shall either be implemented or have a recorded justification in the phase that owns it.

## 114.3 Document Precedence

Where this document conflicts with `CLAUDE.md`, `docs/architecture.md`, or any code comment, **this document wins**. Code comments cite SRS requirement IDs precisely so that drift is detectable — a comment citing an ID that no longer matches this text is a defect in the comment, not in this document.

## 114.4 Updating This Document

A change to this document requires:

1. A revision-history entry stating what changed and why.
2. A check that no existing requirement ID was renumbered.
3. An update to the traceability matrix (§118) for any new or changed requirement.
4. For a change to a security requirement (NFR-SEC-*, §91, §92), a recorded security review.

---

# 115. Assumptions and Dependencies

## 115.1 Assumptions

These are taken as true for the product to deliver its primary objective. **Each is a risk if false** — see §116.

| ID | Assumption | Impact if false |
|---|---|---|
| A-01 | Both computers have Thunderbolt 3, 4, or 5 controllers, not merely USB-C ports. **FALSE on the primary development hardware (2026-10-09).** The Windows endpoint has no Thunderbolt or USB4 controller; its USB-C ports are USB 3.2 Gen 1 (5 Gbps). See §116.1.1. | Realized. No cable network path. Product degrades to Ethernet/Wi-Fi. |
| A-02 | The cable is Thunderbolt-rated or USB4-rated, not a charge-only or USB-2.0 cable. **Not exercised (2026-10-09).** No Thunderbolt-capable pair existed on which to test it. | No cable network path. |
| A-03 | The operating system exposes a Thunderbolt-backed network interface usable for IP traffic. **FALSE on the Windows endpoint (2026-10-09)** — no interface exists because no controller exists. **Unresolved on the Linux endpoint** — a controller is present but the Thunderbolt bus does not come up. See §116.1.1. | Realized. No cable network path. |
| A-04 | The user can grant the one-time OS firewall/network permission the application requests on first run. | Peer discovery and inbound connections fail. |
| A-05 | Local filesystems support files of the sizes targeted (100 GB+, 1 TB+). | Large-file requirements (FR-TRANSFER-006) unverifiable. |
| A-06 | The user has physical access to both machines and can plug a cable between them. | The entire cable-first premise does not apply. |
| A-07 | Development machines have a working Rust toolchain and Tauri system dependencies. | Nothing in `src-tauri/` can be built or verified. **Satisfied on the primary development machine as of 2026-10-08:** rustup 1.99.0 (stable-x86_64-pc-windows-msvc), MSVC Build Tools 2022, Windows SDK, WebView2 154. |

## 115.2 Dependencies

| ID | Dependency | Used for | Risk if unavailable |
|---|---|---|---|
| D-01 | `mdns-sd` crate | mDNS/DNS-SD discovery (FR-DISC-002) | Fall back to link-local UDP discovery (FR-DISC-003), which is required anyway. |
| D-02 | `if-addrs` crate | Interface enumeration (FR-TRANS-001) | Platform-specific enumeration code required. |
| D-03 | `chacha20poly1305`, and a Noise or TLS 1.3 implementation | Session encryption (NFR-SEC-001) | **Blocking.** No custom crypto is permitted as a substitute. |
| D-04 | Tauri 2 runtime and system webview | Desktop shell | **Blocking.** |
| D-05 | Windows code-signing certificate | Signed Windows builds (§84) | Unsigned builds trigger SmartScreen warnings. |
| D-06 | Apple Developer account and notarization | Signed/notarized macOS builds (§84) | Builds are blocked by Gatekeeper on other machines. |
| D-07 | OS-protected secret storage (DPAPI, Keychain, Secret Service) | Private key storage (FR-PAIR-009) | Key must be stored with reduced protection; a security review is required. |

## 115.3 Platform Support Boundaries

The cable objective is not equally achievable on all three platforms. This is stated plainly so that expectations match reality:

| Platform | Thunderbolt networking support | Confidence |
|---|---|---|
| macOS | Thunderbolt Bridge; requires controller firmware support | Medium — works on many machines, historically inconsistent |
| Windows | Requires Thunderbolt Network Service and driver support | Low–Medium on consumer hardware; higher on business/workstation hardware |
| Linux | Least supported; frequently not exposed at all | Low |

**FR-LINK-010:** The application shall not market, document, or display Thunderbolt transfer as a guaranteed capability on any platform. It shall report what it detects on the specific pair of machines in front of it.

---

# 116. Risk Register

Likelihood (L) and Impact (I) are rated High / Medium / Low. **Severity** is the combined rating that determines priority.

## 116.1 Product-Fatal Risks

| ID | Risk | L | I | Severity | Mitigation | Trigger to act |
|---|---|---|---|---|---|---|
| R-01 | **Thunderbolt networking is not available on the target hardware.** The premise (A-01–A-03) is false for the primary development hardware pair. | — | — | **MATERIALIZED** (2026-10-09) | Resolved empirically before `link.rs` was written, as the mitigation required. Evidence and consequences in §116.1.1. | **Triggered.** Both endpoints probed with the cable connected; no Thunderbolt network path at either end. |
| R-02 | ~~The Rust core has never been compiled.~~ **Resolved 2026-10-08.** All three CI gates now pass locally on rustup 1.99.0 (stable-x86_64-pc-windows-msvc): `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test` (**92/92, zero warnings**). Defects found and fixed on first compile: one borrow-checker conflict in `discovery.rs` (E0499/E0502), one clippy `identity_op` lint in `protocol.rs`, and repo-wide `rustfmt` drift across 9 files. | — | — | **Resolved** | Retained here because *compiling is not functioning*: `link.rs`, `discovery.rs`, and `transport.rs` still contain `TODO(Phase …)` stubs, so no requirement is verified by this result. Require green CI before further core work. | CI red on `main`. |
| R-03 | **Pairing/session crypto is designed incorrectly.** Underspecified handshake is the classic source of catastrophic flaws. | Medium | High | **High** | Use a documented handshake pattern (Noise or TLS 1.3) from an established library. No custom cryptography (NFR-SEC-004). Security review before Phase 7 ships. | Any proposal to hand-roll a primitive or a handshake step. |

### 116.1.1 R-01 — Resolution: premise disproven on the primary hardware (2026-10-09)

The cable check required by R-01's mitigation was run on both machines with a USB-C cable connected between them. **Result: no network path, at either end.**

**Windows endpoint** — host `INC-00247`, ASUS Vivobook S 14 S3407CA:

| Observation | Result |
|---|---|
| Thunderbolt / USB4 device in the PnP tree | **None** — searched by friendly name *and* by hardware ID |
| USB silicon | `Intel(R) USB 3.20 eXtensible Host Controller` ×2 — plain xHCI only |
| `Usb4HostRouter` / `Usb4DeviceRouter` drivers | Present but **Stopped** — no device to bind to |
| Devices reporting an error (`ConfigManagerErrorCode != 0`) | **None** — nothing blocked, disabled, or failing |
| Vendor specification | 2× USB 3.2 Gen 1 Type-C (**5 Gbps**), Thunderbolt: **No** |
| Effect of inserting the cable | **Zero change** — no new device, no new interface, no error |

The absence is hardware, not policy. Device-installation restrictions are not even configured on this machine (`DeviceInstall\Restrictions` is unset), and a device-control policy cannot remove a soldered PCI device from enumeration — a blocked controller would still appear, carrying an error code. It does not appear at all. The 5 Gbps port rating corroborates this independently of Windows.

**Linux endpoint** — Arch, kernel 7.1.5-arch1-1:

| Observation | Result |
|---|---|
| Thunderbolt controller | **Present** — `Intel JHL6340 Thunderbolt 3 Bridge (Alpine Ridge 2C 2016)` at `03:00.0`, plus `JHL6340 Thunderbolt 3 USB 3.1 Controller` at `3b:00.0` |
| Thunderbolt bus | **Absent** — `/sys/bus/thunderbolt` does not exist; the `thunderbolt` module is not loaded (only `intel_wmi_thunderbolt`, the firmware control interface) |
| NHI (the PCI function the `thunderbolt` driver binds to) | **Not present** in the PCI device list — consistent with Thunderbolt disabled in firmware |
| Type-C port state | No `typec` ports registered; **no Type-C partner detected** |
| Other interfaces | Onboard Ethernet `enp0s31f6` present but `DOWN`; Wi-Fi `wlp2s0` up |

**Consequences.**

1. The Windows endpoint **can never** participate in a Thunderbolt link. There is no controller to pair, and no OS-level or firmware-level change can create one.
2. The Linux endpoint **can** participate in principle, but has no Thunderbolt peer on the current hardware, and its Thunderbolt bus does not currently come up.
3. The product's headline capability — cable-speed transfer over Thunderbolt — **cannot be demonstrated or delivered on this hardware pair.**

This does **not** invalidate the product:

- The requirement set already specifies honest classification of the non-Thunderbolt case (FR-LINK-003) and transport fallback (FR-TRANS-006). The design anticipated this outcome; the hardware does not match the premise.
- The transfer engine is transport-agnostic. Framing (§18.4), chunking (§21), resume (§22), and integrity verification (§23) are identical over any TCP transport. Nothing in those areas is invalidated.

**Requirements that must change as a result.**

- §3 (Scope), §5 (Objectives), and §99 (Performance) currently state cable-speed transfer as the primary objective. They must be revised to state it as **conditional on hardware capability**, with Ethernet/Wi-Fi as the guaranteed baseline.
- FR-LINK-010 already forbids marketing that guarantees Thunderbolt. The scope and objective language must now be brought into line with it.

**Secondary finding (relevant to FR-DISC-003/004).** The Linux endpoint's firewall was observed dropping IGMP membership traffic bound for the mDNS group (`224.0.0.251`) and the all-hosts group (`224.0.0.1`). Multicast group membership being blocked is precisely the condition that makes mDNS-based discovery fail, and is empirical evidence for why the router-free link-local fallback (FR-DISC-004) is required rather than optional.

## 116.2 Delivery Risks

| ID | Risk | L | I | Severity | Mitigation |
|---|---|---|---|---|---|
| R-04 | Router-free discovery blocked by host firewall on first run. | Medium | High | **High** | FR-DISC-003 link-local fallback; actionable diagnostic (§35, §41); never silently disable the firewall. |
| R-05 | Cable unplug mid-chunk corrupts or loses data. | Medium | High | **High** | Chunk atomicity (FR-CHUNK-001); a partial chunk is never persisted or counted as complete. |
| R-06 | Scope creep pulls the product back toward a general-purpose network transfer tool. | Medium | Medium | **Medium** | §6 non-goals are binding for the initial release; every new requirement must answer the §1 question. |
| R-07 | Cross-platform filesystem metadata loss (permissions, timestamps, symlinks). | Medium | Low | **Low** | FR-FS-* requirements; unsupported metadata must not fail the transfer. |
| R-08 | macOS signing/notarization blocks distribution. | Medium | Low | **Low** | D-06 acquired before Phase 10, not during it. |
| R-09 | Performance target unreachable because storage, not network, is the bottleneck. | Medium | Medium | **Medium** | Report measured throughput (§101); never advertise link speed as transfer speed. |
| R-10 | Resume metadata corrupts across application restart or OS sleep. | Low | High | **Medium** | FR-RESUME-004; state written atomically; incomplete transfers never shown as complete (§50). |

## 116.3 Risk Review Cadence

R-01 and R-02 shall be re-evaluated at the end of every phase in §108. R-01 in particular can invalidate the product premise and must be resolved empirically — by running the cable check — before significant engineering effort is spent on the cable path.

**R-01 was re-evaluated on 2026-10-09 and materialized** (§116.1.1). R-02 was resolved on 2026-10-08 (A-07). Both are now closed; the remaining product-fatal risk is R-03. R-01 shall be re-opened if the target hardware changes to a pair with Thunderbolt controllers on both ends.

---

# 117. Verification Methods

Every requirement is verified by one or more of the following methods. The method for each critical requirement is recorded in §118.

| Code | Method | Meaning |
|---|---|---|
| **T** | Test | Executed and observed objectively — unit, integration, or end-to-end. Preferred method. |
| **D** | Demonstration | Observed operating in a realistic scenario (e.g., two real machines and a real cable). |
| **A** | Analysis | Verified by reasoning, modelling, or measurement rather than direct execution (e.g., crypto review, throughput modelling). |
| **I** | Inspection | Verified by reading the artifact (e.g., confirming no custom crypto primitives are present). |

## 117.1 Method by Requirement Class

| Requirement class | Primary method | Rationale |
|---|---|---|
| FR-LINK-*, FR-TRANS-* | **D** | Depends on real hardware and OS behaviour; cannot be unit-tested. |
| FR-DISC-* | **T** + **D** | Loopback tests for protocol; real-cable demonstration for the no-router case. |
| FR-PAIR-*, NFR-SEC-* | **T** + **A** | Tests for the state machine; analysis and review for the cryptographic design. |
| FR-TRANSFER-*, FR-CHUNK-*, FR-RESUME-* | **T** | Deterministic and reproducible on a loopback transport. |
| NFR-PERF-* | **D** + **A** | Measured on representative hardware; modelled where hardware is unavailable. |
| NFR-SEC-004 (no custom crypto) | **I** | Inspection of dependencies and diff review. |
| Packaging, signing (§83, §84) | **D** | Install and launch the produced artifact on a clean machine. |

## 117.2 Verification Status

Verification status is tracked per requirement as **Unverified → In Progress → Verified → Failed**. A requirement may not be marked Verified without a recorded method, a reproducible procedure, and an observed result.

**As of 2026-10-08, every requirement in this document is still Unverified.** The core now compiles and its 92 unit tests pass (R-02 resolved), but those tests exercise data structures and pure logic — they do not exercise a cable, a peer, or a real transfer. No requirement in §118 may move to Verified until the hardware-dependent work in Phases 2–4 is implemented and demonstrated.

---

# 118. Requirements Traceability Matrix

Maps each critical and high-priority requirement to its development phase (§108) and its verification method (§117). Phases are those defined in §108.

| Requirement | Priority | Phase | Verification | Status |
|---|---|---|---|---|
| FR-LINK-001 Cable capability detection | Critical | 2 | D | Unverified — **R-01 materialized**; must now detect and report the negative case (§116.1.1) |
| FR-LINK-003 No false Thunderbolt claims | Critical | 2 | D, T | Unverified — **now the primary requirement**; the target hardware is the negative case (§116.1.1) |
| FR-LINK-010 No guaranteed-Thunderbolt marketing | Critical | 9 | I | Unverified |
| FR-DISC-003 mDNS-independent discovery | Critical | 3 | T, D | Unverified |
| FR-DISC-004 Router-free discovery | Critical | 3 | D | Unverified — R-04 |
| FR-DISC-010 Discovery rate limiting | High | 3 | T | Unverified |
| FR-PAIR-001 Device authorization | Critical | 7 | T | Unverified |
| FR-PAIR-003 Verification code match | Critical | 7 | T | Unverified |
| FR-PAIR-005 Pairing rate limiting | High | 7 | T | Unverified |
| FR-PAIR-009 OS-protected key storage | Critical | 7 | I, D | Unverified |
| FR-TRANS-001 Interface detection | Critical | 2 | D | Unverified |
| FR-TRANS-006 Transport fallback | High | 2 | D | Unverified |
| FR-SESS-002 Explicit role model | Critical | 4 | T | Unverified |
| FR-TRANSFER-001 File transfer | Critical | 4 | T, D | Unverified |
| FR-TRANSFER-002 Folder transfer | Critical | 5 | T | Unverified |
| FR-TRANSFER-005 Streaming | Critical | 4 | T | Unverified |
| FR-TRANSFER-006 Large files (100 GB+) | Critical | 4 | D | Unverified |
| FR-CHUNK-001 Chunk atomicity | Critical | 6 | T | Unverified |
| FR-RESUME-001 Resume | Critical | 6 | T | Unverified |
| FR-RESUME-004 Resume survives restart | Critical | 6 | T | Unverified |
| NFR-SEC-001 Encrypted transfers | Critical | 7 | A, T | Unverified |
| NFR-SEC-004 No custom crypto | Critical | 7 | I | Unverified |
| §89 Path traversal rejection | Critical | 4 | T | Unverified |
| §92 Malicious input handling | Critical | 4 | T | Unverified |
| NFR-PERF-005 Multi-GB/s throughput | High | 8 | D, A | Unverified — R-09 |
| §83 Installer works on clean machine | Critical | 10 | D | Unverified |

## 118.1 Traceability Gaps

This matrix is deliberately incomplete: it covers the critical and high-priority requirements, not all of them. As the remaining requirements are implemented, each gains a row. A requirement with no phase, no verification method, or no status is a defect in this document, not an optional entry.

---

# 119. v3.0 Baseline Summary

Version 3.0 makes **no change to any requirement's meaning**. It is a formalisation pass: it declares the language rules (§114), states the assumptions the product rests on (§115), tracks the hardware premise as a named risk (§116), assigns a verification method to each requirement class (§117), and provides a traceability matrix (§118).

The single most important statement in this revision is **R-01**:

> The product's central premise — that a Thunderbolt cable between two computers yields a usable network path — is **assumed, not yet demonstrated**. It must be resolved empirically, by running `scripts/verify-cable.sh` on both machines with the real cable, before significant engineering effort is invested in the cable path.

If R-01 resolves negative on the target hardware, the product is still viable, but its headline capability becomes **honest classification and graceful fallback** rather than cable-speed transfer — and §3, §5, and §99 would need to be revised accordingly.

---

## 119.1 Amendment — R-01 resolved negative (2026-10-09)

The condition in the paragraph above has occurred. The cable check was run on both machines with the cable connected, and **R-01 materialized**: the Windows endpoint has no Thunderbolt or USB4 controller at all, and the Linux endpoint's Thunderbolt 3 controller has no Thunderbolt peer and no bus. Full evidence is recorded in **§116.1.1**.

**No requirement's meaning changed in this amendment.** What changed is the status of the premise the requirements rest on:

| Item | Before 2026-10-09 | After |
|---|---|---|
| A-01 | Assumed | **False** on the primary hardware |
| A-02 | Assumed | Not exercised |
| A-03 | Assumed | **False** on Windows; unresolved on Linux |
| R-01 | Critical, open | **Materialized**, closed |
| R-02 | Resolved (2026-10-08) | Unchanged |
| Product-fatal risks open | R-01, R-03 | **R-03 only** |

The consequence stated above now applies: the headline capability is **honest classification and graceful fallback**, and §3, §5, and §99 required revision to stop presenting cable-speed transfer as the guaranteed primary objective. **That revision has since landed — see §119.2.**

This document retains the version identifier 3.0 rather than incrementing, because the requirement baseline is unchanged — only a tracked risk's status and the assumptions that fed it have moved. The file was not renamed, so existing references to `FastTransfer_SRS_v3.0.md` remain valid.

---

## 119.2 Amendment — objective restated as hardware-conditional (2026-10-09)

The revision §119.1 called for is complete. Cable-speed Thunderbolt transfer is no longer stated anywhere as the guaranteed primary objective; it is stated as a conditional capability that the product must deliver when the hardware permits and must honestly decline when it does not.

| Section | Before | After |
|---|---|---|
| §2 Product Vision | "transfer at cable speed" | "transfer as fast as the link allows", with the Thunderbolt case named as the fast one |
| §3 Core Objective | cable-speed Thunderbolt transfer, unconditional | direct, router-free, verified transfer over the best available path (**guaranteed**); cable speed over Thunderbolt when both endpoints support it (**conditional**); new §3.1 records why |
| §5 Goals | Thunderbolt listed among primary goals; fallback secondary | §5.1 primary/guaranteed (fallback and honest classification promoted into it); §5.2 new conditional Thunderbolt goal; §5.3 secondary |
| §99 Version 1.0 | `✓ Thunderbolt / USB4 transport`, undifferentiated | `~ Thunderbolt / USB4 transport — conditional on hardware (§5.2)`, with an explicit note that v1.0 acceptance is measured against §5.1 |
| §106 Acceptance Criteria | "A Thunderbolt 3 cable between two machines is classified as `THUNDERBOLT`"; connectivity required "when cabled" | the positive case is qualified to machines that expose a Thunderbolt network interface; the **negative** case gains its own criterion; the router-free requirement is scoped to the direct-cable case |

**No requirement identifier changed, and no requirement was weakened.** FR-LINK-001 and FR-LINK-003 already required detecting and honestly reporting the non-Thunderbolt case (§118); what changed is that the document now states plainly that this is the case the reference hardware presents, and stops treating the Thunderbolt path as the bar v1.0 must clear on any machine.

The one substantive editorial risk this removes: §106 previously contained an acceptance criterion that could not be satisfied on hardware the product must support. Leaving it would have made "v1.0 complete" unreachable and, worse, would have tempted an implementer to satisfy it by misreporting the link — the exact failure FR-LINK-003 exists to prevent.

---

## 119.3 Amendment — Windows link probe implemented at device level (2026-10-09)

The Windows arm of `link.rs::observe()` no longer matches on a network adapter's display name. It enumerates the device tree through SetupAPI (`SetupDiGetClassDevsW` + `SPDRP_*`) and classifies on device-bound evidence, with the alias used only as the join key between the device tree and the live interface list.

**What identifies Thunderbolt-family hardware on Windows** — established by measurement and by Microsoft's USB4 connection-manager documentation, not by assumption:

| Signal | Evidence | Strength |
|---|---|---|
| Bound driver service `Usb4HostRouter` | Host router; Microsoft's USB4 connection manager | Device-bound |
| Bound driver service `Usb4P2PNetAdapter` | The host-to-host network adapter — the actual cable path | Device-bound |
| Hardware/compatible ID `PCI\CC_0C0340` | USB4 host interface class code | Device-bound |
| Hardware ID prefix `USB4\` | USB4 enumerator | Device-bound |
| Device class `USB4` or `Thunderbolt` | Device class name | Device-bound |
| Device name contains "Thunderbolt" | INF-supplied; the only signal a discrete Windows 10 controller offers | Capability only, never classification |

### Correction: `PCI\CC_0880` is not a Thunderbolt marker

The first implementation of this probe, and step 1 of `verify-cable.ps1`, both treated PCI class code `CC_0880` as evidence of a Thunderbolt controller. **That is wrong, and it was caught by running the probe on the reference machine**, where it reported `thunderbolt_capable: true` on hardware §116.1.1 had established has no Thunderbolt controller at all.

The matching device was the **Intel GNA Scoring Accelerator** (`PCI\VEN_8086&DEV_774C&CC_0880`). `CC_0880` is the generic PCI class 08/80 ("Base System Peripheral / Other") compatible ID; unrelated System devices carry it. Intel's Thunderbolt INF does list `...&CC_0880` entries, but always bound to a specific device ID, which is why the class code alone proves nothing. `CC_0C0340` — Serial Bus / USB / USB4 host interface — is specific and is what the probe uses.

This is recorded because it is a trap: a class code that *appears* in Thunderbolt driver-matching tables and is in fact shared with unrelated hardware. `verify-cable.ps1` step 1 was corrected in the same change, and carries the warning inline.

### Consequence for §116.1.1

The live probe on the Windows reference machine now returns:

```text
LinkObservation { cable_present: false, thunderbolt_network: false,
                  usb4_network: false, usb_data: false,
                  thunderbolt_capable: false } -> None
```

This confirms §116.1.1's finding by a stronger method. That finding rested on `Get-PnpDevice | Where FriendlyName -match 'Thunderbolt|USB4'` — itself a name filter, and so vulnerable to exactly the blind spot corrected here. The device-tree probe reaches the same conclusion from hardware IDs and driver bindings, which is independent evidence and does not depend on any name.

### What remains unverified

- **Thunderbolt 3 on Windows has no distinguishing PCI class code.** Detection rests on the device class name and the INF-supplied device name, both of which a Windows 10 discrete controller does provide but which this project has no hardware to test against. A miss degrades to the alias fallback — never to a false `THUNDERBOLT` claim, which is the safe direction (FR-LINK-003). FR-LINK-001 and FR-LINK-003 therefore remain **Unverified** in §118 until exercised on real Thunderbolt hardware.
- **The Windows join is still by display name**, because that is what `if-addrs` exposes; correlating an interface to its PnP instance requires `GetAdaptersAddresses` and a `NetCfgInstanceId` lookup, which is not implemented. The *evidence* is device-bound, so a renamed adapter is identified; an adapter the device tree cannot name falls back to the alias heuristic.
- **macOS still reports `thunderbolt_capable` as `false`**, needing an IOKit probe that is not implemented. The effect is confined to the remedy text.
