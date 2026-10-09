# FastTransfer
# Software Requirements Specification (SRS)

**Document Version:** 2.0
**Status:** Product Specification / Baseline
**Supersedes:** FastTransfer Full SRS v1.0
**Application Type:** Cross-platform desktop application
**Target Platforms:** Windows, Linux, macOS
**Frontend:** React + Vite + TypeScript
**Desktop Framework:** Tauri 2
**Native Core:** Rust
**Primary Objective:** Direct computer-to-computer file transfer over a Thunderbolt 3 / USB-C cable

---

# 1. Document Purpose

This document defines the complete Software Requirements Specification for **FastTransfer**, a cross-platform desktop application for transferring files and folders directly between two computers.

Version 2.0 sharpens the product around a single, concrete objective:

> **Transfer files between two computers over a Thunderbolt 3 / USB-C cable, at the highest speed the hardware and cable can actually sustain.**

The v1.0 specification described a general-purpose transfer engine that *preferred* Thunderbolt among several transports. This revision inverts that priority. The **cable is the product**. Ethernet and Wi-Fi remain supported, but they exist as fallbacks for when a cable is not present — not as co-equal paths.

Everything in this document is subordinate to one question: *does this make cable transfer faster, more reliable, or easier to use?*

---

# 2. Product Vision

> Plug a cable between two computers → both see each other → pick files → transfer at cable speed.

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

**Move data between two computers over a Thunderbolt 3 (USB-C) cable, using the Thunderbolt networking data path exposed by the operating system, with no router, no switch, no Internet connection, and no cloud service involved.**

Success for this objective means:

1. A user connects two computers with a Thunderbolt 3 / USB4 cable.
2. Each computer detects the other automatically.
3. The user authorizes the peer once.
4. Files transfer at a rate limited by the cable, the storage, and the CPU — not by the application.
5. The transfer is verified, resumable, and encrypted.

Everything else in this SRS supports that objective.

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

The system shall:

- Transfer files between two supported computers.
- Transfer complete folders.
- Detect and use Thunderbolt networking when a Thunderbolt link is present.
- Automatically discover the peer on the direct cable link, **without requiring a router or DHCP server**.
- Establish a direct connection with no Internet dependency.
- Encrypt all transfers.
- Require explicit user authorization for a new peer.
- Verify transferred data with SHA-256.
- Support resumable transfers after interruption.
- Stream large files with bounded memory.
- Report **measured** throughput, not theoretical link speed.
- Support Windows, Linux, and macOS.
- Provide a polished desktop UI.
- Provide production-ready installers.

## 5.2 Secondary Goals

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
✓ Thunderbolt / USB4 transport
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
```

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

- A Thunderbolt 3 cable between two machines is classified as `THUNDERBOLT`.
- A USB data-only cable is classified as `USB_DATA_ONLY` and does not claim Thunderbolt.
- A charge-only cable is classified as `CHARGE_ONLY`.
- In every case, the application explains the result in user terms.

## Connectivity

- Windows ↔ Windows, Windows ↔ Linux, Windows ↔ macOS
- Linux ↔ Linux, Linux ↔ macOS
- macOS ↔ macOS
all work **with no router, no DHCP, and no Internet** when cabled.

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
