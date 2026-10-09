# F12 — Packaging, CI and release

| | |
|---|---|
| **Status** | Draft |
| **SRS phase** | 10 (§108), with CI from Phase 1 |
| **SRS sources** | §82, §83, §84, §85, §86, §87, §88, §107, D-04, D-05, D-06, R-08, A-07 |
| **Depends on** | all |
| **Verdict** | VALID WITH CHANGES |
| **Last validated** | 2026-11-09 |

## 1. Intent

Every change is built and tested on every supported platform, and a release is a
signed installer that works on a clean machine.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| §83 | MSI + EXE; AppImage, DEB, RPM; DMG signed and notarised | — (no ID) | VALID | Partial: bundle targets configured, never built in CI by default |
| §84 | Code signing per platform | — | VALID | Not started (D-05, D-06 not acquired) |
| §85 | CI builds Windows, Linux, **macOS Intel and Apple Silicon**; lint → typecheck → tests → integration → build → package → security checks | shall | VALID | **Gap:** `macos-latest` is Apple Silicon only; no integration or security-check stage |
| §86 | Unit, integration and end-to-end testing incl. a real cable | — | VALID | Partial: unit tests; loopback integration tests arrive with F05 |
| §87 | 9-pair cross-platform matrix | — | **VALID WITH CHANGES** — finding 1 | Not started |
| §88 | Per-transport matrix incl. data-only and charge-only cables | — | **NEEDS CHANGE** — finding 2 | Not started |
| §82 | Signed auto-update | should (future) | VALID | Deferred |

## 3. Validity findings

1. **There is no macOS reference machine.** §115 lists a Windows and a Linux
   endpoint only. Five of the nine §87 pairs, and every macOS-specific
   requirement (FR-LINK-004, §8.3), cannot be demonstrated. The SRS does not
   record this as an assumption or risk. → **CR-13**.
2. **§88's cable rows inherit F01 finding 1.** "USB data-only cable (must classify
   correctly…)" and "Charge-only cable (must classify correctly…)" cannot pass on
   host-to-host links that the OS does not report. → CR-02.
3. **The CI bundle job runs only on manual dispatch**, so "a build that compiles
   is not a build that installs" is checked only when someone remembers.

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F12-1 | macOS Intel | Add a `macos-13` (Intel) runner alongside `macos-latest`. | Proposed |
| D-F12-2 | Security checks | `cargo audit` and `npm audit --omit=dev` as a CI job; fail on known-vulnerable dependencies. | Proposed — new tool |
| D-F12-3 | Bundles | Run the bundle job on every push to `main` (not only on dispatch); keep publishing manual (`scripts/deploy.sh`). | Proposed |
| D-F12-4 | Real-cable runs | A manual checklist in `docs/` per §87/§88 row, recording date, machines, cable, result — the "recorded procedure" §117.2 requires. | Proposed |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F12-01 | Every push runs fmt, clippy (warnings denied), Rust tests, typecheck, lint and Vitest on Windows, Linux and macOS | §85 | I | `.github/workflows/ci.yml` | Passing (Intel macOS missing) |
| AC-F12-02 | CI includes a macOS Intel build | §85 | I | `ci.yml` | Planned (D-F12-1) |
| AC-F12-03 | CI fails on a known-vulnerable dependency | §85 | I + T | planned CI job | Planned |
| AC-F12-04 | Each platform's installer installs, launches and uninstalls on a clean machine | §83 | D | Manual | Planned (macOS blocked by CR-13) |
| AC-F12-05 | Release builds are signed (and notarised on macOS) | §84 | D | Manual | Blocked on D-05, D-06 |
| AC-F12-06 | Each §87 pair and §88 row has a dated, recorded result | §117.2 | D | `docs/` checklist | Planned |

## 6. Out of scope

- Auto-update (§82) — future release.

## 7. Current implementation

- [`.github/workflows/ci.yml`](../.github/workflows/ci.yml): frontend job;
  Rust job on ubuntu/macos/windows with fmt, clippy `-D warnings`, tests; bundle
  job on `workflow_dispatch` only.
- [`tauri.conf.json`](../src-tauri/tauri.conf.json): bundle targets `msi`,
  `nsis`, `deb`, `rpm`, `appimage`, `dmg`, `app`; `signingIdentity: null`.
- [`scripts/deploy.sh`](../scripts/deploy.sh), [`Dockerfile`](../Dockerfile)
  (Linux build environment).
