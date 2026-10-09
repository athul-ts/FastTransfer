# FastTransfer

Cable-first direct file transfer between two computers.

Plug a Thunderbolt 3 / USB-C cable between two machines, and move files at cable
speed — no router, no DHCP, no Internet, no cloud.

## Status

Early development. See [`FastTransfer_SRS_v3.0.md`](FastTransfer_SRS_v3.0.md) for
the requirements baseline, and SRS §112 for the first engineering milestone.

## Why

Existing transfer tools assume a network. FastTransfer assumes a **cable**. It:

- detects what a plugged cable can actually do, and says so honestly
- discovers the peer with no router and no DHCP server present
- prefers Thunderbolt, falls back to Ethernet, then Wi-Fi
- reports *measured* throughput, never the link rating
- resumes across cable unplug and replug
- verifies every transfer with SHA-256

## The one thing to understand

A USB-C **connector** is not a capability. Only a Thunderbolt cable between two
machines with Thunderbolt controllers, on an OS that exposes the networking path,
gives you a transfer link. FastTransfer classifies the link and reports one of:

```text
THUNDERBOLT      → use it
USB4_NETWORK     → use it
USB_DATA_ONLY    → explain, fall back to Ethernet/Wi-Fi
CHARGE_ONLY      → explain, fall back to Ethernet/Wi-Fi
NONE             → fall back to Ethernet/Wi-Fi
```

It will never claim Thunderbolt because it saw a USB-C connector.

## Stack

| Layer | Technology |
|---|---|
| Frontend | React 18 + TypeScript + Vite 6 |
| UI | Tailwind CSS + shadcn/ui |
| Desktop | Tauri 2 |
| Core | Rust |
| Protocol | TCP, length-prefixed |
| Integrity | SHA-256 |

## Prerequisites

- Node.js 20+
- Rust 1.77+ (`rustup`)
- Tauri 2 system dependencies — see <https://tauri.app/start/prerequisites/>

## Getting started

```bash
./scripts/setup.sh        # checks prerequisites, installs deps, runs both type-checkers
npm run tauri dev
```

Frontend only (no Rust core):

```bash
npm run dev
```

**Before writing any transfer code, run the cable check on both machines:**

```bash
./scripts/verify-cable.sh
```

It is read-only, and it answers the question the whole product depends on: does
*this* cable, on *this* hardware, on *this* OS, actually give you a network path?
Run it on both ends and compare the output. On most consumer hardware the answer
is no — see SRS §4.

## Scripts

| Command | Purpose |
|---|---|
| `npm run tauri dev` | Run the full desktop app |
| `npm run dev` | Frontend only, in a browser |
| `npm run build` | Type-check and build the frontend |
| `npm run tauri build` | Produce a production installer |
| `npm test` | Frontend tests (Vitest) |
| `npm run typecheck` | TypeScript, no emit |
| `npm run lint` | ESLint |
| `cd src-tauri && cargo test` | Rust tests |
| `cd src-tauri && cargo clippy` | Rust lints |

## Project structure

```text
src/            React UI
src-tauri/      Rust core
tests/          Frontend tests
docs/           Architecture and API reference
scripts/        Setup and build helpers
```

See [`CLAUDE.md`](CLAUDE.md) for conventions and [`docs/onboarding.md`](docs/onboarding.md)
to get productive quickly.

## Development phases

See SRS §108. The first milestone (SRS §112) is a Rust proof of concept that
classifies the cable, discovers the peer with no router, transfers 1 GB, and
verifies SHA-256 on both ends — **before any UI work**.

## License

TBD.
