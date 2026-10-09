# Onboarding

Get productive on FastTransfer in an hour.

## 1. Understand what the app actually does

It moves files between two computers over a **Thunderbolt 3 / USB-C cable**,
with no router, no DHCP, and no Internet.

The single most important thing to internalise, because it shapes half the code:

> A USB-C **connector** is not a capability. Only a Thunderbolt cable between two
> machines whose OS exposes the networking path gives you a transfer link.

`link.rs` classifies a plugged cable as `THUNDERBOLT`, `USB4_NETWORK`,
`USB_DATA_ONLY`, `CHARGE_ONLY`, or `NONE`, and the app must **never** claim
Thunderbolt because it saw a USB-C connector. That is SRS §4 and FR-LINK-003.

## 2. Read, in this order

| # | File | Why |
|---|---|---|
| 1 | `README.md` | what it is, how to run it |
| 2 | `CLAUDE.md` | conventions, security rules, git workflow |
| 3 | `docs/architecture.md` | module map and the two invariants |
| 4 | `FastTransfer_SRS_v3.0.md` §4, §17 | the cable, precisely |
| 5 | `FastTransfer_SRS_v3.0.md` §91, §92 | the threat model |
| 6 | `docs/api-reference.md` | the IPC contract |

Do not read the SRS end to end on day one. It is a reference, not a tutorial.

## 3. Get it running

```bash
npm install
npm run tauri dev
```

Prerequisites: Node 20+, Rust 1.77+, and the Tauri 2 system dependencies for
your platform — <https://tauri.app/start/prerequisites/>.

On Linux you need WebKitGTK **4.1**, not 4.0. This trips up most first builds:

```bash
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev
```

Or use the container:

```bash
docker build -t fasttransfer-build .
docker run --rm -v "$PWD:/app" fasttransfer-build npm run tauri build
```

## 4. Run the tests

```bash
npm test                                  # frontend, Vitest
npm run typecheck                         # TypeScript, no emit
cd src-tauri && cargo test                # Rust core
cd src-tauri && cargo clippy --all-targets -- -D warnings
```

The Rust tests are where the value is. Start with `filesystem.rs` — the test
module there is a reading list of everything that can go wrong with a
peer-supplied path.

## 5. The two things you must not break

Read these before your first real change.

### `filesystem::safe_join`

Every path from the peer goes through it. It rejects `..`, absolute paths,
Windows drive and UNC prefixes, reserved device names, and trailing dots. It is
*deliberately* stricter than a lenient implementation would be.

If your change adds a write path that does not go through `prepare` →
`safe_join`, it is a vulnerability. Add the test to `filesystem.rs` first.

### Chunk atomicity

A chunk is complete only after its digest matches. A partial chunk is never
recorded. Resume depends on this entirely — if it breaks, transfers "succeed"
with holes in them.

## 6. Where to start contributing

Work in phase order (SRS §108). The early phases are the ones that de-risk the
product:

| Phase | Work | Module |
|---|---|---|
| 2 | Cable classification on each OS | `link.rs` — `observe()` |
| 2 | Interface enumeration | `transport.rs` — `enumerate_interfaces()` |
| 3 | mDNS registration and browse | `discovery.rs` — `start()` |
| 3 | Link-local probe (no router) | `discovery.rs` — `probe_link_local()` |
| 4 | The transfer engine itself | `transfer.rs` |

Every one of those is a `TODO(Phase N)` in the code with the SRS section
attached. Grep for `TODO(Phase` to see the current frontier:

```bash
grep -rn "TODO(Phase" src-tauri/src/
```

## 7. The first milestone

SRS §112: **a Rust proof of concept, before any UI work.**

```text
Laptop A ──[ Thunderbolt 3 cable ]── Laptop B

1.  classify the link
2.  enumerate interfaces, find the Thunderbolt path
3.  discover the peer with no router and no DHCP
4.  connect
5.  authenticate
6.  transfer 1 GB
7.  measure actual throughput
8.  SHA-256 both ends, verify
9.  disconnect cleanly
```

Then repeat with a USB data-only cable — it must classify as `USB_DATA_ONLY` and
refuse to transfer — and with no router present.

Everything else is built on that validated foundation. If Thunderbolt networking
turns out not to work on your hardware, you want to know now, not after the UI.

## 8. Debugging

```bash
FASTTRANSFER_LOG=debug npm run tauri dev      # verbose core logging
FASTTRANSFER_DISCOVERY=linklocal npm run tauri dev   # force the fallback path
```

Logs never contain file contents or key material. If you need more detail than
`trace` gives you, fix the logging, do not add a `println!` with a path in it.

## 9. Getting help

- The SRS is authoritative for scope. If code and the SRS disagree, that is a bug
  in one of them — say so rather than picking a side silently.
- `CLAUDE.md` has the conventions and the non-negotiable security rules.
- `/review` and `/test-all` in Claude Code run the project's own review and test
  workflows.
