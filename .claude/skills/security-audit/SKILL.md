---
name: security-audit
description: Audit FastTransfer for vulnerabilities at the hostile-peer boundary — path traversal, malicious metadata, DoS, and cryptographic misuse. Use when the user asks for a security review or audit, before a release, or after changes to protocol.rs, filesystem.rs, security.rs, or discovery.rs.
---

# Security Audit

FastTransfer accepts data from a peer that is authenticated but **not trusted**.
A paired device can still send malformed input by accident or on purpose. This
skill audits the code that faces that peer.

## Threat model

Assume the remote peer:
- has passed pairing (or is trying to)
- controls every byte on the wire
- controls every filename, path, size, count, and hash it sends
- may be a legitimate user's buggy client or an attacker on the same link

Do not assume a device is safe because it was discovered on the cable. SRS §92.

## Checklist

### Path traversal — SRS §91, the critical one

- [ ] Every write path is derived through `filesystem::safe_join`.
- [ ] The check is on the final resolved path, after normalization and symlink
      resolution — not on the raw wire string.
- [ ] `..`, absolute paths, drive-relative paths, and UNC prefixes are all handled.
- [ ] Windows reserved device names cannot be created.
- [ ] Symlinks inside the destination cannot be used to escape it.
- [ ] Unicode normalization cannot produce two paths that alias to one file.

### Malicious metadata — SRS §92

- [ ] Every wire-supplied length is validated before it sizes an allocation or a `Vec::with_capacity`.
- [ ] Chunk indices are bounds-checked against the declared file size.
- [ ] Declared file size is checked against available disk space before transfer starts.
- [ ] Transfer IDs from the peer cannot collide with or overwrite local transfer state.
- [ ] Protocol version mismatch is rejected, not coerced.

### Denial of service — SRS §93

- [ ] Discovery is rate-limited; a flood of advertisements cannot saturate the UI or the CPU.
- [ ] `MAX_MESSAGE_SIZE` is enforced on read, before buffering.
- [ ] The queue of pending peering requests is bounded.
- [ ] Stream count is capped (SRS FR-PAR-001).
- [ ] A peer that connects and sends nothing is timed out.

### Cryptography — SRS §34, NFR-SEC-009

- [ ] No hand-rolled primitive, mode, KDF, or handshake anywhere.
- [ ] The session handshake uses a documented pattern (Noise `XX` or TLS 1.3)
      from an established, maintained crate.
- [ ] Nonces are never reused with the same key.
- [ ] Private keys live in OS secure storage (Keychain / DPAPI / Secret Service)
      where available — never in a plain config file.
- [ ] Verification codes are single-use and time-limited (FR-PAIR-004).
- [ ] Pairing attempts are rate-limited (FR-PAIR-005).

### Leakage — NFR-SEC-007, §71

- [ ] No file contents in logs, at any level, including `TRACE`.
- [ ] No key material, pairing secret, or session key in logs or error messages.
- [ ] File names do not leave the two participating devices.
- [ ] Crash reports and telemetry carry no file metadata.

### Tauri surface — SRS §81

- [ ] `src-tauri/capabilities/*.json` grants the minimum permissions.
- [ ] Unrestricted filesystem access is not exposed to the webview.
- [ ] No `dangerousDisableAssetCspModification` / CSP relaxation without a reason.

## Reporting

For each finding: severity, `file:line`, the attack, and the concrete impact.
Rank Critical → High → Medium → Low.

- **Critical**: remote code execution, arbitrary file write outside the
  destination, key exfiltration, silent data corruption.
- **High**: DoS of the whole app, path escape limited to a subdirectory,
  unauthenticated transfer.
- **Medium**: information disclosure of local paths, resource exhaustion
  with a bounded blast radius.
- **Low**: hardening and defence in depth.

Do not report a missing `#[deny]` or a style issue as a vulnerability. If the
audit finds nothing at a severity, state that explicitly rather than inventing a
finding to fill the section.
