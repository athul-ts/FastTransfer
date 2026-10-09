#!/usr/bin/env bash
#
# First-time development setup: verify prerequisites, install dependencies, and
# sanity-check the two toolchains.
#
# Idempotent — safe to re-run after a toolchain upgrade.

set -euo pipefail

RED=$'\033[0;31m'
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
NC=$'\033[0m'

ok()   { printf '%s✓%s %s\n' "$GREEN" "$NC" "$1"; }
warn() { printf '%s!%s %s\n' "$YELLOW" "$NC" "$1"; }
fail() { printf '%s✗%s %s\n' "$RED" "$NC" "$1"; exit 1; }

printf '\nFastTransfer — development setup\n\n'

# --- Node -------------------------------------------------------------------

if ! command -v node >/dev/null 2>&1; then
  fail "Node.js not found. Install Node 20+ from https://nodejs.org"
fi

NODE_MAJOR=$(node -p 'process.versions.node.split(".")[0]')
if [ "$NODE_MAJOR" -lt 20 ]; then
  fail "Node $NODE_MAJOR found; FastTransfer needs 20 or newer."
fi
ok "Node $(node -v)"

# --- Rust -------------------------------------------------------------------

if ! command -v cargo >/dev/null 2>&1; then
  fail "Rust not found. Install with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

RUST_VERSION=$(rustc --version | awk '{print $2}')
RUST_MAJOR=$(echo "$RUST_VERSION" | cut -d. -f1)
RUST_MINOR=$(echo "$RUST_VERSION" | cut -d. -f2)
if [ "$RUST_MAJOR" -eq 1 ] && [ "$RUST_MINOR" -lt 77 ]; then
  fail "Rust $RUST_VERSION found; FastTransfer needs 1.77 or newer."
fi
ok "Rust $RUST_VERSION"

# --- Platform dependencies --------------------------------------------------

case "$(uname -s)" in
  Linux)
    # WebKitGTK 4.1, not 4.0. Tauri 2 requires the 4.1 series and the older
    # package produces a confusing link error rather than a clear one.
    if ! pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
      warn "libwebkit2gtk-4.1-dev not found. Install it before building:"
      printf '    sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev \\\n'
      printf '      libayatana-appindicator3-dev librsvg2-dev\n'
    else
      ok "WebKitGTK 4.1 present"
    fi
    ;;
  Darwin)
    if ! xcode-select -p >/dev/null 2>&1; then
      warn "Xcode command line tools missing. Run: xcode-select --install"
    else
      ok "Xcode command line tools present"
    fi
    ;;
  MINGW*|MSYS*|CYGWIN*)
    ok "Windows detected — WebView2 ships with Windows 11"
    ;;
esac

# --- Dependencies -----------------------------------------------------------

printf '\nInstalling npm dependencies…\n'
npm install

printf '\nFetching Rust crates…\n'
cargo fetch --manifest-path src-tauri/Cargo.toml

# --- Environment ------------------------------------------------------------

if [ ! -f .env ]; then
  cp .env.example .env
  ok "Created .env from .env.example"
else
  ok ".env already exists (left untouched)"
fi

# --- Verification -----------------------------------------------------------

printf '\nVerifying the toolchains build…\n'
npm run typecheck
cargo check --manifest-path src-tauri/Cargo.toml

printf '\n%sSetup complete.%s\n\n' "$GREEN" "$NC"
printf 'Next:\n'
printf '  npm run tauri dev                     # run the app\n'
printf '  cd src-tauri && cargo test            # Rust tests\n'
printf '  npm test                              # frontend tests\n\n'
printf 'The first engineering milestone is SRS §112 — a Rust proof of concept\n'
printf 'that classifies a real cable and transfers 1 GB across it.\n\n'
