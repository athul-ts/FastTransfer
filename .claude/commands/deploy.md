---
description: Produce signed release bundles for all target platforms
allowed-tools: Bash
---

# Release

Build production installers. This does not publish anything — it produces
artifacts for a human to review and sign off on.

## Preflight

Do not proceed if any of these fail. Report and stop.

```bash
npm run typecheck
npm test
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
```

Confirm the version in `package.json`, `src-tauri/Cargo.toml`, and
`src-tauri/tauri.conf.json` all agree. Mismatched versions produce installers
that cannot be updated.

## Build

```bash
npm run tauri build
```

Artifacts land in `src-tauri/target/release/bundle/`.

Expected per platform (SRS §83):

| Platform | Artifacts |
|---|---|
| Windows | `.msi`, `.exe` (NSIS) |
| Linux | `.AppImage`, `.deb`, `.rpm` |
| macOS | `.dmg`, `.app` |

## Verify before declaring success

- Every expected artifact exists and is non-zero.
- macOS and Windows builds are signed; an unsigned artifact is a failure, not a
  warning (SRS §84).
- The Linux AppImage launches on a clean machine without Node or Rust installed.
- Report artifact paths and sizes.

## Do not

- Push a git tag, create a GitHub release, or upload anywhere unless explicitly
  asked in this session.
