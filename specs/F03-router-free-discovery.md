# F03 — Router-free peer discovery

| | |
|---|---|
| **Status** | Approved 2026-11-09 — Implemented (T) 2026-11-09 — D criteria pending |
| **SRS phase** | 3 (§108) |
| **SRS sources** | §13 (FR-DISC-001…010), §14, §72, §73, §93, §116.1.1 secondary finding, D-01 |
| **Depends on** | F02 |
| **Verdict** | VALID |
| **Last validated** | 2026-11-09 |

## 1. Intent

Two computers joined by a cable, with no router, no DHCP and no Internet, see each
other within seconds. This is a **guaranteed** goal (§5.1), and unlike the
Thunderbolt path it can be demonstrated on the reference hardware: over a direct
Ethernet cable (with a USB Ethernet adapter if the Windows laptop has no port).
It is the single most valuable feature not yet built.

## 2. Requirements in scope

| SRS ID | Requirement (short) | Keyword | Verdict | Implementation |
|---|---|---|---|---|
| FR-DISC-001 | Discover peers on direct and local links | shall | VALID | Partial: peer table only; nothing finds peers |
| FR-DISC-002 | Use mDNS/DNS-SD where available | shall | VALID | Not started (`mdns-sd` is a dependency) |
| FR-DISC-003 | Do not depend on mDNS; link-local fallback | shall | VALID | Not started (`probe_link_local` returns `PeerNotFound`) |
| FR-DISC-004 | Work with no DHCP; IPv4 and IPv6 link-local | shall | VALID | Not started |
| FR-DISC-005 | Advertise id, name, version, OS, protocol, transports, link capability | should | VALID | Partial: TXT lacks transport types and link capability |
| FR-DISC-006 | Never auto-authorise a discovered device | shall | VALID | Holds trivially (no transfer path exists yet) |
| FR-DISC-007 | Skip virtual interfaces | shall | VALID | Implemented in F02 |
| FR-DISC-008 | Update the list as devices appear and disappear | shall | VALID | Not started: `device_*` events are never emitted, though `App.tsx` listens |
| FR-DISC-009 | Never expose file names or private paths | shall | VALID | Implemented (`txt_record_carries_no_file_information`) |
| FR-DISC-010 | Rate-limit discovery | shall | VALID | Implemented in the peer table; not yet wired to a socket |

## 3. Validity findings

1. **IPv6 link-local is the address a bare Thunderbolt link is guaranteed to
   have.** Linux brings up `thunderbolt0` with only `fe80::/64` unless
   NetworkManager assigns more. An IPv6 link-local address is useless without its
   interface index (scope id), so the peer record must keep a scoped socket
   address internally, not just the `IpAddr` shown in the UI. Not stated in the
   SRS; recorded as D-F03-3.
2. **Multicast can be blocked.** §116.1.1 recorded the Linux endpoint's firewall
   dropping IGMP for `224.0.0.251` and `224.0.0.1`. That breaks mDNS *and* any
   IPv4-multicast fallback. The fallback must include IPv4 limited broadcast
   (`255.255.255.255`), which needs no group membership. → D-F03-1.
3. **"Supported transport types, link capability" (FR-DISC-005)** has no defined
   encoding. → D-F03-4.
4. **Discovery is unauthenticated by nature.** Anything on the link can claim any
   device id and name. FR-DISC-006 and NFR-SEC-002 are therefore load-bearing:
   identity is only established by F08. The UI must not show a discovered name as
   trusted. (Consistent with the SRS; recorded so it is tested.)

## 4. Design decisions

| ID | Question | Recommendation | Status |
|---|---|---|---|
| D-F03-1 | Fallback mechanism | UDP beacon every 3 s (`PROBE_INTERVAL`) carrying the FR-DISC-005 fields as JSON (≤ 1 KiB): IPv4 to `255.255.255.255` and each interface's directed broadcast; IPv6 to `ff02::1` on each interface by index. Receivers reply by unicast. | Accepted 2026-11-09 |
| D-F03-2 | Ports | TCP 47820 for transfers. UDP 47820 for IPv4 beacons, UDP 47821 for IPv6 beacons — separate ports avoid the dual-stack bind conflict between `0.0.0.0` and `[::]` on Linux. | Accepted 2026-11-09 |
| D-F03-3 | Peer addressing | `Peer.addresses` stays display-only. Discovery keeps an internal endpoint list per peer: scoped `SocketAddr`, the local interface it was seen on, and that interface's transport kind. | Accepted 2026-11-09 |
| D-F03-4 | Advertising transports | Add TXT keys `tr` (e.g. `thunderbolt,ethernet`) and `link` (the F01 class). Same fields in the beacon. Add `transport` to `Peer` (IPC change, TS mirror). | Accepted 2026-11-09 |
| D-F03-5 | Liveness | Drop a peer after 30 s unheard (`PEER_TTL`), emit `device_removed`. Emit `device_discovered` on first sight and `device_updated` when any field changes. | Accepted 2026-11-09 |
| D-F03-6 | Untrusted beacon content | Cap display name at 64 characters, strip control characters, cap device id at 64 ASCII characters; drop malformed or oversized beacons; never reflect a beacon's claimed address — use the packet's source address. | Accepted 2026-11-09 |
| D-F03-7 | Self-discovery | Ignore beacons carrying our own device id. | Accepted 2026-11-09 |

## 5. Acceptance criteria

| ID | Given / When / Then | SRS | Method | Test | Status |
|---|---|---|---|---|---|
| AC-F03-01 | The advertisement never contains file or path information | FR-DISC-009 | T | `discovery::tests::txt_record_carries_no_file_information` | Passing |
| AC-F03-02 | The advertisement contains id, name, OS, app version, protocol, port | FR-DISC-005 | T | `txt_record_includes_the_required_fields` | Passing |
| AC-F03-03 | A flood from one source is capped; other sources are unaffected | FR-DISC-010 | T | `flood_from_one_source_is_dropped`, `rate_limiter_is_per_source` | Passing |
| AC-F03-04 | Given two discovery instances on loopback with mDNS disabled, when beacons run, then each lists the other within 5 s | FR-DISC-003 | T | `discovery::tests::beacons_find_a_peer_without_mdns` | Passing (uncommitted) |
| AC-F03-05 | Given a beacon with an oversized, malformed or control-character name, then it is dropped or sanitised and nothing panics | FR-DISC-010, §92 | T | `discovery::tests::hostile_beacons_are_refused_by_the_parser`, `beacon_names_are_sanitised`, `hostile_datagrams_do_not_reach_the_peer_list` | Passing (uncommitted) |
| AC-F03-06 | Given an IPv6 link-local beacon, then the stored endpoint carries the receiving interface's scope id | FR-DISC-004 | T | `discovery::tests::ipv6_link_local_endpoint_is_scoped`, `ipv4_endpoint_is_matched_by_subnet` | Passing (uncommitted) |
| AC-F03-07 | Given a peer unheard for 30 s, then it is removed and `device_removed` is emitted | FR-DISC-008 | T | `discovery::tests::silent_peers_expire_with_an_event` | Passing (uncommitted) |
| AC-F03-08 | Our own beacon never lists ourselves | — | T | `discovery::tests::own_beacon_is_ignored` | Passing (uncommitted) |
| AC-F03-09 | A discovered peer is shown as not trusted until F08 pairing completes | FR-DISC-006, NFR-SEC-002 | T | `discovery::tests::discovered_peers_are_untrusted` | Passing (uncommitted) |
| AC-F03-10 | Given Windows ↔ Linux on a direct Ethernet cable, no router, no DHCP, multicast blocked on Linux, then both see each other within 10 s | FR-DISC-003, FR-DISC-004, §73 | D | Manual | Planned — **feasible on the reference hardware** |
| AC-F03-11 | Same, over a Thunderbolt cable | FR-DISC-004 | D | Manual | **CONDITIONAL (§5.2)** |

## 6. Out of scope

- Proving who a peer is → F08.
- Firewall diagnosis (FR-FW-001) → F11.

## 7. Current implementation

**Progress 2026-11-09 (uncommitted):** UDP beacons on IPv4 broadcast and IPv6 `ff02::1` (ports 47820/47821), unicast reply to a new probe, mDNS registration and browse through the same parser, peer TTL with `device_*` events wired to the window, scoped IPv6 endpoints, virtual-interface rejection, peer-table bound (256). AC-F03-01…09 pass. Not yet demonstrated: AC-F03-10 (direct Ethernet, no router) and AC-F03-11 (Thunderbolt, conditional).

- [`discovery.rs`](../src-tauri/src/discovery.rs): `Advertisement` and TXT
  encoding, `RateLimiter`, shared peer table with TTL, `Discovery::start` (no-op),
  `probe_link_local` (stub), `DiscoveryMode::from_env` (never called).
- Missing: everything that touches the network, the endpoint list, events.
