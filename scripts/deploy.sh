#!/usr/bin/env bash
#
# Produce release installers for the current platform.
#
# This builds and verifies artifacts. It does NOT publish anything — no tags, no
# releases, no uploads. Those stay manual.

set -euo pipefail

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
NC=$'\033[0m'

step() { printf '\n%s==>%s %s\n' "$GREEN" "$NC" "$1"; }
die()  { printf '%s✗%s %s\n' "$RED" "$NC" "$1" >&2; exit 1; }

step "Preflight — nothing ships unless these pass"

npm run typecheck   || die "TypeScript check failed"
npm test            || die "Frontend tests failed"
cargo test --manifest-path src-tauri/Cargo.toml || die "Rust tests failed"
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings \
  || die "Clippy found warnings"

# Version skew produces installers that cannot be updated in place, which is a
# failure mode users hit months later. Catch it now.
step "Checking version agreement"

PKG_VERSION=$(node -p "require('./package.json').version")
CARGO_VERSION=$(grep -m1 '^version' src-tauri/Cargo.toml | cut -d'"' -f2)
TAURI_VERSION=$(node -p "require('./src-tauri/tauri.conf.json').version")

if [ "$PKG_VERSION" != "$CARGO_VERSION" ] || [ "$PKG_VERSION" != "$TAURI_VERSION" ]; then
  die "Version mismatch: package.json=$PKG_VERSION Cargo.toml=$CARGO_VERSION tauri.conf.json=$TAURI_VERSION"
fi
printf 'Version %s in all three manifests\n' "$PKG_VERSION"

step "Building"
npm run tauri build

BUNDLE_DIR="src-tauri/target/release/bundle"

step "Artifacts"

if [ ! -d "$BUNDLE_DIR" ]; then
  die "No bundle directory at $BUNDLE_DIR — the build did not produce installers"
fi

FOUND=0
while IFS= read -r artifact; do
  FOUND=$((FOUND + 1))
  SIZE=$(du -h "$artifact" | cut -f1)
  printf '  %-64s %s\n' "${artifact#"$BUNDLE_DIR/"}" "$SIZE"
done < <(find "$BUNDLE_DIR" -type f \( -name '*.msi' -o -name '*.exe' -o -name '*.dmg' \
  -o -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) | sort)

if [ "$FOUND" -eq 0 ]; then
  die "No installers found under $BUNDLE_DIR"
fi

# Signing is a requirement, not a nicety (SRS §84). Report status rather than
# failing, because unsigned local builds are legitimate during development —
# but they must never be published.
case "$(uname -s)" in
  Darwin)
    APP=$(find "$BUNDLE_DIR/macos" -maxdepth 1 -name '*.app' | head -n1 || true)
    if [ -n "$APP" ]; then
      step "Signature"
      if codesign --verify --deep --strict "$APP" 2>/dev/null; then
        printf '  Signed and valid\n'
      else
        printf '  %sUNSIGNED — notarisation required before release%s\n' "$RED" "$NC"
      fi
    fi
    ;;
esac

printf '\n%sBuilt %d artifact(s).%s\n' "$GREEN" "$FOUND" "$NC"
printf 'Nothing was published. Review the artifacts, then release manually.\n\n'
