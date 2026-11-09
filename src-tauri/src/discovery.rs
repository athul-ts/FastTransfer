//! Peer discovery, with a router-free fallback.
//!
//! Implements SRS §13 and spec F03. The requirement that shapes this module is
//! FR-DISC-003/004: **discovery must work on a direct cable with no router and
//! no DHCP server.**
//!
//! That rules out relying on mDNS alone. On a bare cable link there is usually
//! no mDNS responder, multicast group membership can be firewalled (§116.1.1
//! recorded IGMP being dropped), and the two machines have link-local addresses
//! (`169.254.0.0/16`, `fe80::/10`) that neither side has advertised. So the
//! module runs two strategies and merges their results by device id:
//!
//! | Strategy | Works when | Fails when |
//! |---|---|---|
//! | UDP beacons (D-F03-1) | there is a direct L2 path; needs no group membership for IPv4 | the host firewall drops inbound UDP |
//! | mDNS / DNS-SD | both hosts have working multicast | no responder, or multicast filtered |
//!
//! Everything a beacon says is untrusted (§92). The parser bounds its size,
//! validates the id and port, and sanitises display text; the endpoint is taken
//! from the packet's source address, never from anything the peer claims; and
//! the transport is judged from *our* interface the packet arrived on.
//! Discovery never implies trust (FR-DISC-006): `Peer::trusted` is always false
//! here.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::link::{self, InterfaceKind, LinkKind};
use crate::protocol::{self, Capabilities, PROTOCOL_VERSION};
use crate::transport::{self, LocalAddr, TransportKind};

/// The DNS-SD service type. `_fasttransfer._tcp.local.`
pub const SERVICE_TYPE: &str = "_fasttransfer._tcp.local.";

/// Default port for the transfer listener (TCP).
pub const DEFAULT_PORT: u16 = 47820;

/// UDP port for IPv4 beacons (D-F03-2). Same number as the TCP listener, which
/// is a different protocol and does not conflict.
pub const BEACON_PORT_V4: u16 = 47820;

/// UDP port for IPv6 beacons. Kept apart from the IPv4 port because on Linux a
/// `[::]` socket is dual-stack and would collide with `0.0.0.0` (D-F03-2).
pub const BEACON_PORT_V6: u16 = 47821;

/// Largest beacon accepted or sent. A legitimate one is a few hundred bytes.
pub const MAX_BEACON_BYTES: usize = 1024;

/// Beacon format version. A different version is refused, not guessed at.
const BEACON_VERSION: u8 = 1;

/// How long a peer may go unheard before it is dropped (D-F03-5).
const PEER_TTL: Duration = Duration::from_secs(30);

/// Beacon cadence. 3 s is fast enough that a bare cable feels instant and slow
/// enough that it never becomes the flood FR-DISC-010 exists to prevent.
const PROBE_INTERVAL: Duration = Duration::from_secs(3);

/// How often the local interface list is re-read, so a cable plugged in after
/// start-up gets beacons too.
const LOCAL_REFRESH_INTERVAL: Duration = Duration::from_secs(10);

/// How often expired peers are swept.
const EXPIRY_INTERVAL: Duration = Duration::from_secs(2);

/// Bounds on what one peer, and all peers, can make us store (SRS §93).
const MAX_ENDPOINTS_PER_PEER: usize = 8;
const MAX_PEERS: usize = 256;

/// `ff02::1`, the IPv6 all-nodes group on each link.
const ALL_NODES: Ipv6Addr = Ipv6Addr::new(0xff02, 0, 0, 0, 0, 0, 0, 1);

/// A peer as shown in the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub platform_version: String,
    pub app_version: String,
    pub protocol_version: String,
    /// What the peer advertised. The capabilities a session actually uses are
    /// negotiated in the handshake (F04), never taken from discovery.
    pub capabilities: Capabilities,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    /// How we found it. Shown in diagnostics so a failure is explainable.
    pub via: DiscoveryMethod,
    /// The cable class of the local interface the peer was heard on.
    pub link: LinkKind,
    /// The best transport this machine has to the peer, judged from our own
    /// interface — never from the peer's claim (FR-LINK-003, D-F03-4).
    pub transport: Option<TransportKind>,
    /// Always false here; only pairing (F08) establishes trust (FR-DISC-006).
    pub trusted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiscoveryMethod {
    /// Found via mDNS/DNS-SD.
    Mdns,
    /// Found by a link-local UDP beacon.
    LinkLocalProbe,
}

/// This device's own advertisement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advertisement {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub platform_version: String,
    pub app_version: String,
    pub protocol_version: String,
    pub capability_flags: Vec<String>,
    pub port: u16,
    /// Transport kinds this machine can offer, best first (FR-DISC-005).
    pub transports: Vec<String>,
    /// This machine's own cable classification (FR-DISC-005). The receiver
    /// shows it but never uses it to classify its own link.
    pub link: LinkKind,
}

impl Advertisement {
    pub fn new(device_id: String, display_name: String, platform: String, port: u16) -> Self {
        Self {
            device_id,
            display_name,
            platform,
            platform_version: String::new(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            protocol_version: PROTOCOL_VERSION.to_string(),
            capability_flags: vec!["resume".into(), "sha256".into()],
            port,
            transports: Vec::new(),
            link: LinkKind::None,
        }
    }

    /// Sets the advertised transports and link class.
    pub fn with_link(mut self, transports: Vec<String>, link: LinkKind) -> Self {
        self.transports = transports;
        self.link = link;
        self
    }

    /// Encodes to the TXT record pairs mDNS expects.
    ///
    /// Only the FR-DISC-005 fields are included. Adding a filename here would
    /// broadcast it to the whole link against FR-DISC-009 — do not.
    pub fn to_txt_pairs(&self) -> Vec<(String, String)> {
        vec![
            ("id".into(), self.device_id.clone()),
            ("name".into(), self.display_name.clone()),
            ("os".into(), self.platform.clone()),
            ("osv".into(), self.platform_version.clone()),
            ("app".into(), self.app_version.clone()),
            ("proto".into(), self.protocol_version.clone()),
            ("caps".into(), self.capability_flags.join(",")),
            ("port".into(), self.port.to_string()),
            ("tr".into(), self.transports.join(",")),
            ("link".into(), link_label(self.link).into()),
        ]
    }

    /// The UDP beacon form of this advertisement. `reply` marks an answer to
    /// someone else's beacon, so the receiver does not answer it in turn.
    pub fn beacon(&self, reply: bool) -> Beacon {
        Beacon {
            v: BEACON_VERSION,
            id: self.device_id.clone(),
            name: self.display_name.clone(),
            os: self.platform.clone(),
            osv: self.platform_version.clone(),
            app: self.app_version.clone(),
            proto: self.protocol_version.clone(),
            port: self.port,
            tr: self.transports.clone(),
            link: link_label(self.link).into(),
            reply,
        }
    }
}

/// The wire name of a link class, matching `LinkKind`'s serde form.
fn link_label(kind: LinkKind) -> &'static str {
    match kind {
        LinkKind::Thunderbolt => "THUNDERBOLT",
        LinkKind::Usb4Network => "USB4_NETWORK",
        LinkKind::UsbDataOnly => "USB_DATA_ONLY",
        LinkKind::ChargeOnly => "CHARGE_ONLY",
        LinkKind::None => "NONE",
    }
}

/// One UDP discovery datagram (D-F03-1). JSON, at most [`MAX_BEACON_BYTES`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    pub v: u8,
    pub id: String,
    pub name: String,
    pub os: String,
    #[serde(default)]
    pub osv: String,
    pub app: String,
    pub proto: String,
    /// The peer's TCP transfer port.
    pub port: u16,
    #[serde(default)]
    pub tr: Vec<String>,
    #[serde(default)]
    pub link: String,
    #[serde(default)]
    pub reply: bool,
}

/// Parses and validates a beacon from the wire.
///
/// Refuses anything oversized, malformed, of another version, with an id that
/// is not UUID-shaped, or with port 0. Display fields are sanitised and capped
/// rather than refused, so a peer with an odd name still appears (D-F03-6).
pub fn parse_beacon(bytes: &[u8]) -> Result<Beacon> {
    if bytes.len() > MAX_BEACON_BYTES {
        return Err(Error::MessageTooLarge {
            size: bytes.len() as u64,
            limit: MAX_BEACON_BYTES as u32,
        });
    }
    let mut beacon: Beacon = serde_json::from_slice(bytes)
        .map_err(|e| Error::MalformedMessage(format!("beacon: {e}")))?;

    if beacon.v != BEACON_VERSION {
        return Err(Error::MalformedMessage(format!(
            "beacon version {} is not supported",
            beacon.v
        )));
    }
    if !protocol::is_valid_device_id(&beacon.id) {
        return Err(Error::MalformedMessage(
            "beacon device id is malformed".into(),
        ));
    }
    if beacon.port == 0 {
        return Err(Error::MalformedMessage("beacon port is 0".into()));
    }

    beacon.name = protocol::sanitize_remote_text(&beacon.name, protocol::MAX_DISPLAY_NAME_CHARS);
    if beacon.name.is_empty() {
        beacon.name = "Unknown device".into();
    }
    beacon.os = protocol::sanitize_remote_text(&beacon.os, 32);
    beacon.osv = protocol::sanitize_remote_text(&beacon.osv, 64);
    beacon.app = protocol::sanitize_remote_text(&beacon.app, 32);
    beacon.proto = protocol::sanitize_remote_text(&beacon.proto, 16);
    beacon.link = protocol::sanitize_remote_text(&beacon.link, 32);
    beacon.tr = beacon
        .tr
        .iter()
        .take(8)
        .map(|t| protocol::sanitize_remote_text(t, 16))
        .filter(|t| !t.is_empty())
        .collect();
    Ok(beacon)
}

/// Somewhere a peer can be dialled, and what kind of path it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The packet's source address on the peer's advertised TCP port. An IPv6
    /// link-local address keeps the scope id of the interface it arrived on,
    /// without which it cannot be dialled (D-F03-3).
    pub addr: SocketAddr,
    /// The local interface the peer was heard on, when we could tell.
    pub interface: Option<String>,
    pub transport: Option<TransportKind>,
    pub link: LinkKind,
}

/// Ranking used to order endpoints and peers: the F02 class order.
fn transport_rank(t: Option<TransportKind>) -> u8 {
    match t {
        Some(TransportKind::Thunderbolt) => 3,
        Some(TransportKind::Ethernet) => 2,
        Some(TransportKind::Wifi) => 1,
        None => 0,
    }
}

fn is_ipv6_link_local(ip: &Ipv6Addr) -> bool {
    (ip.segments()[0] & 0xffc0) == 0xfe80
}

/// Whether `ip` is on the same subnet as the local address.
fn same_subnet(local: &LocalAddr, ip: IpAddr) -> bool {
    match (local.ip, ip) {
        (IpAddr::V4(mine), IpAddr::V4(theirs)) => {
            let prefix = u32::from(local.prefix_len.min(32));
            let mask = if prefix == 0 {
                0
            } else {
                u32::MAX << (32 - prefix)
            };
            (u32::from(mine) & mask) == (u32::from(theirs) & mask)
        }
        (IpAddr::V6(mine), IpAddr::V6(theirs)) if !is_ipv6_link_local(&theirs) => {
            let prefix = u32::from(local.prefix_len.min(128));
            let mask = if prefix == 0 {
                0
            } else {
                u128::MAX << (128 - prefix)
            };
            (u128::from(mine) & mask) == (u128::from(theirs) & mask)
        }
        _ => false,
    }
}

/// Where to dial a peer heard from `src`, judged by our interface it arrived on.
///
/// Returns `None` when that interface is virtual: a VPN or container bridge is
/// not a path to the computer on the other end of the cable (FR-DISC-007).
pub fn endpoint_for(src: SocketAddr, tcp_port: u16, locals: &[LocalAddr]) -> Option<Endpoint> {
    // A v4-mapped address from a dual-stack socket is really IPv4.
    let src = match src {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::V4(SocketAddrV4::new(v4, v6.port())),
            None => src,
        },
        v4 => v4,
    };

    let local = match src {
        SocketAddr::V6(v6) if v6.scope_id() != 0 => {
            locals.iter().find(|l| l.index == Some(v6.scope_id()))
        }
        _ => locals.iter().find(|l| same_subnet(l, src.ip())),
    };
    if local.is_some_and(|l| l.is_virtual) {
        return None;
    }

    let addr = match src {
        SocketAddr::V4(v4) => SocketAddr::V4(SocketAddrV4::new(*v4.ip(), tcp_port)),
        SocketAddr::V6(v6) => {
            SocketAddr::V6(SocketAddrV6::new(*v6.ip(), tcp_port, 0, v6.scope_id()))
        }
    };
    let link = match local.map(|l| l.kind) {
        Some(InterfaceKind::Thunderbolt) => LinkKind::Thunderbolt,
        Some(InterfaceKind::Usb4Network) => LinkKind::Usb4Network,
        _ => LinkKind::None,
    };
    Some(Endpoint {
        addr,
        interface: local.map(|l| l.name.clone()),
        transport: local.and_then(LocalAddr::transport),
        link,
    })
}

/// Where this machine sends its periodic beacons when no explicit targets are
/// configured: the IPv4 limited broadcast, each interface's directed
/// broadcast (Windows sends the limited broadcast out of one interface only),
/// and `ff02::1` on every IPv6 link-local interface by index.
fn broadcast_targets(locals: &[LocalAddr]) -> Vec<SocketAddr> {
    let mut out = vec![SocketAddr::from((Ipv4Addr::BROADCAST, BEACON_PORT_V4))];
    for l in locals.iter().filter(|l| !l.is_virtual && l.up) {
        match l.ip {
            IpAddr::V4(ip) if l.prefix_len < 31 => {
                let directed = l.broadcast.unwrap_or_else(|| {
                    let host_bits = u32::MAX >> u32::from(l.prefix_len.min(32));
                    Ipv4Addr::from(u32::from(ip) | host_bits)
                });
                out.push(SocketAddr::from((directed, BEACON_PORT_V4)));
            }
            IpAddr::V6(ip) if is_ipv6_link_local(&ip) => {
                if let Some(index) = l.index {
                    out.push(SocketAddr::V6(SocketAddrV6::new(
                        ALL_NODES,
                        BEACON_PORT_V6,
                        0,
                        index,
                    )));
                }
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Joins `ff02::1` on every IPv6 link-local interface. Repeated joins fail
/// harmlessly; this runs on every beacon tick so a newly plugged link is
/// covered without a restart.
fn join_all_nodes(socket: &UdpSocket, locals: &[LocalAddr]) {
    for l in locals.iter().filter(|l| !l.is_virtual) {
        if let (IpAddr::V6(ip), Some(index)) = (l.ip, l.index) {
            if is_ipv6_link_local(&ip) {
                let _ = socket.join_multicast_v6(&ALL_NODES, index);
            }
        }
    }
}

/// The transport kinds a set of local addresses offers, best first.
fn advertised_transports(locals: &[LocalAddr]) -> Vec<String> {
    let mut kinds: Vec<TransportKind> = locals.iter().filter_map(LocalAddr::transport).collect();
    kinds.sort_by_key(|k| std::cmp::Reverse(transport_rank(Some(*k))));
    kinds.dedup();
    kinds
        .into_iter()
        .map(|k| match k {
            TransportKind::Thunderbolt => "thunderbolt".to_string(),
            TransportKind::Ethernet => "ethernet".to_string(),
            TransportKind::Wifi => "wifi".to_string(),
        })
        .collect()
}

/// Something about the peer list changed (FR-DISC-008, D-F03-5).
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    Discovered(Peer),
    Updated(Peer),
    Removed { device_id: String },
}

/// Receives discovery events; the app forwards them to the UI.
pub type EventSink = Arc<dyn Fn(DiscoveryEvent) + Send + Sync>;

/// Rate limiter for inbound discovery traffic (SRS FR-DISC-010, §93).
///
/// A peer that floods the link must not be able to saturate the CPU or the UI.
struct RateLimiter {
    per_source: HashMap<String, Vec<Instant>>,
    max_per_window: usize,
    window: Duration,
}

impl RateLimiter {
    fn new(max_per_window: usize, window: Duration) -> Self {
        Self {
            per_source: HashMap::new(),
            max_per_window,
            window,
        }
    }

    fn allow(&mut self, source: &str, now: Instant) -> bool {
        // Hoist the fields out so the closure below does not capture `self`
        // while `per_source` is mutably borrowed.
        let window = self.window;
        let max_per_window = self.max_per_window;

        let allowed = {
            let hits = self.per_source.entry(source.to_string()).or_default();
            hits.retain(|t| now.duration_since(*t) < window);

            if hits.len() >= max_per_window {
                false
            } else {
                hits.push(now);
                true
            }
        };

        // Bound the map itself: an attacker rotating source identities must not
        // grow it without limit. This runs after the entry borrow is released —
        // `entry` and `retain` both borrow `per_source` mutably, so they cannot
        // be held at the same time.
        if self.per_source.len() > 1024 {
            self.per_source.retain(|_, v| !v.is_empty());
        }

        allowed
    }
}

/// One entry in the peer table.
struct PeerEntry {
    peer: Peer,
    seen: Instant,
    /// Best transport first.
    endpoints: Vec<Endpoint>,
    /// Whether we already answered this peer's probe (D-F03-1).
    replied: bool,
}

fn build_peer(beacon: &Beacon, endpoints: &[Endpoint], via: DiscoveryMethod) -> Peer {
    let best = endpoints.first();
    let mut addresses: Vec<IpAddr> = endpoints.iter().map(|e| e.addr.ip()).collect();
    addresses.dedup();
    Peer {
        device_id: beacon.id.clone(),
        display_name: beacon.name.clone(),
        platform: beacon.os.clone(),
        platform_version: beacon.osv.clone(),
        app_version: beacon.app.clone(),
        protocol_version: beacon.proto.clone(),
        capabilities: Capabilities {
            transports: beacon.tr.clone(),
            ..Capabilities::default()
        },
        addresses,
        port: beacon.port,
        via,
        link: best.map(|e| e.link).unwrap_or(LinkKind::None),
        transport: best.and_then(|e| e.transport),
        trusted: false,
    }
}

/// How discovery binds and where it sends. `Default` is production.
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    pub v4_bind: SocketAddr,
    pub v6_bind: Option<SocketAddr>,
    /// Explicit unicast targets. `None` means broadcast and multicast on every
    /// interface, which is what production does.
    pub targets: Option<Vec<SocketAddr>>,
    pub interval: Duration,
    pub beacons: bool,
    pub mdns: bool,
    /// Whether to read the OS interface list (blocking; skipped in tests).
    pub enumerate_interfaces: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        let mode = DiscoveryMode::from_env();
        Self {
            v4_bind: SocketAddr::from((Ipv4Addr::UNSPECIFIED, BEACON_PORT_V4)),
            v6_bind: Some(SocketAddr::from((Ipv6Addr::UNSPECIFIED, BEACON_PORT_V6))),
            targets: None,
            interval: PROBE_INTERVAL,
            beacons: mode != DiscoveryMode::MdnsOnly,
            mdns: mode != DiscoveryMode::LinkLocalOnly,
            enumerate_interfaces: true,
        }
    }
}

impl DiscoveryConfig {
    /// Loopback only, fast cadence, explicit targets, no mDNS.
    #[cfg(test)]
    pub(crate) fn for_test(targets: Vec<SocketAddr>) -> Self {
        Self {
            v4_bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            v6_bind: None,
            targets: Some(targets),
            interval: Duration::from_millis(100),
            beacons: true,
            mdns: false,
            enumerate_interfaces: false,
        }
    }
}

/// The discovery service.
///
/// Cheap to clone, and every clone shares one peer table, one rate limiter and
/// one event sink. That sharing is the point: the background tasks and the IPC
/// layer must see the same device list.
#[derive(Clone)]
pub struct Discovery {
    own_id: Arc<str>,
    advertisement: Arc<std::sync::RwLock<Advertisement>>,
    peers: Arc<RwLock<HashMap<String, PeerEntry>>>,
    /// `Mutex`, not `RwLock`: every access is a short synchronous update, and
    /// the guard is never held across an `await`.
    limiter: Arc<Mutex<RateLimiter>>,
    sink: Arc<std::sync::RwLock<Option<EventSink>>>,
}

impl Discovery {
    pub fn new(advertisement: Advertisement) -> Self {
        Self {
            own_id: Arc::from(advertisement.device_id.as_str()),
            advertisement: Arc::new(std::sync::RwLock::new(advertisement)),
            peers: Arc::new(RwLock::new(HashMap::new())),
            limiter: Arc::new(Mutex::new(RateLimiter::new(30, Duration::from_secs(1)))),
            sink: Arc::new(std::sync::RwLock::new(None)),
        }
    }

    /// A snapshot of this device's current advertisement.
    pub fn advertisement(&self) -> Advertisement {
        match self.advertisement.read() {
            Ok(a) => a.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// Where discovery events go. Replaces any previous sink.
    pub fn set_event_sink(&self, sink: EventSink) {
        match self.sink.write() {
            Ok(mut s) => *s = Some(sink),
            Err(poisoned) => *poisoned.into_inner() = Some(sink),
        }
    }

    fn emit(&self, event: DiscoveryEvent) {
        let sink = match self.sink.read() {
            Ok(s) => s.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(sink) = sink {
            sink(event);
        }
    }

    fn rate_limited(&self, source: &str, now: Instant) -> bool {
        // Poisoning means another thread panicked mid-update. The limiter's
        // data is still sound, so recovering beats taking discovery down.
        let mut limiter = match self.limiter.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        !limiter.allow(source, now)
    }

    /// Starts discovery with the production configuration.
    pub async fn start(&self) -> Result<()> {
        self.start_with(DiscoveryConfig::default())
            .await
            .map(|_| ())
    }

    /// Starts the beacon, mDNS and expiry tasks. Returns the bound IPv4 beacon
    /// address (the configured one when beacons are off).
    pub async fn start_with(&self, config: DiscoveryConfig) -> Result<SocketAddr> {
        info!(
            service = SERVICE_TYPE,
            beacons = config.beacons,
            mdns = config.mdns,
            "starting discovery"
        );

        let locals: Arc<RwLock<Vec<LocalAddr>>> = Arc::new(RwLock::new(Vec::new()));
        if config.enumerate_interfaces {
            self.refresh_locals(&locals).await;
            let this = self.clone();
            let l = locals.clone();
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(LOCAL_REFRESH_INTERVAL);
                tick.tick().await;
                loop {
                    tick.tick().await;
                    this.refresh_locals(&l).await;
                }
            });
        }

        let mut bound = config.v4_bind;
        if config.beacons {
            let v4 = UdpSocket::bind(config.v4_bind).await?;
            v4.set_broadcast(true)?;
            bound = v4.local_addr()?;
            let v4 = Arc::new(v4);

            let v6 = match config.v6_bind {
                Some(addr) => match UdpSocket::bind(addr).await {
                    Ok(s) => Some(Arc::new(s)),
                    Err(e) => {
                        debug!(error = %e, "IPv6 beacons unavailable");
                        None
                    }
                },
                None => None,
            };

            self.spawn_receiver(v4.clone(), locals.clone());
            if let Some(v6) = &v6 {
                self.spawn_receiver(v6.clone(), locals.clone());
            }
            self.spawn_sender(
                v4,
                v6,
                locals.clone(),
                config.targets.clone(),
                config.interval,
            );
        }

        let this = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(EXPIRY_INTERVAL);
            loop {
                tick.tick().await;
                this.expire(Instant::now()).await;
            }
        });

        if config.mdns {
            if let Err(e) = self.start_mdns(locals) {
                warn!(error = %e, "mDNS unavailable; link-local beacons only");
            }
        }

        Ok(bound)
    }

    /// Re-reads interfaces and the link class off the async runtime, and
    /// refreshes what this device advertises.
    async fn refresh_locals(&self, locals: &RwLock<Vec<LocalAddr>>) {
        let result = tokio::task::spawn_blocking(|| {
            let observation = link::observe();
            let kind = link::classify(&observation);
            (transport::local_addrs_with(&observation), kind)
        })
        .await;
        let Ok((addrs, kind)) = result else {
            return;
        };
        let transports = advertised_transports(&addrs);
        match self.advertisement.write() {
            Ok(mut ad) => {
                ad.transports = transports;
                ad.link = kind;
            }
            Err(poisoned) => {
                let mut ad = poisoned.into_inner();
                ad.transports = transports;
                ad.link = kind;
            }
        }
        *locals.write().await = addrs;
    }

    fn spawn_receiver(&self, socket: Arc<UdpSocket>, locals: Arc<RwLock<Vec<LocalAddr>>>) {
        let this = self.clone();
        tokio::spawn(async move {
            // Larger than any valid beacon, so an oversized one is seen and
            // refused rather than silently truncated into something parseable.
            let mut buf = vec![0u8; MAX_BEACON_BYTES * 2];
            loop {
                match socket.recv_from(&mut buf).await {
                    Ok((n, src)) => {
                        let snapshot = locals.read().await.clone();
                        if let Some(reply) = this
                            .handle_datagram(&buf[..n], src, &snapshot, Instant::now())
                            .await
                        {
                            if let Ok(bytes) = serde_json::to_vec(&reply) {
                                let _ = socket.send_to(&bytes, src).await;
                            }
                        }
                    }
                    // Windows reports an ICMP "port unreachable" from an earlier
                    // send as a receive error (WSAECONNRESET). It must not end
                    // the loop; nothing else should either.
                    Err(e) => {
                        debug!(error = %e, "beacon receive failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                }
            }
        });
    }

    fn spawn_sender(
        &self,
        v4: Arc<UdpSocket>,
        v6: Option<Arc<UdpSocket>>,
        locals: Arc<RwLock<Vec<LocalAddr>>>,
        targets: Option<Vec<SocketAddr>>,
        interval: Duration,
    ) {
        let this = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            loop {
                tick.tick().await;
                let Ok(bytes) = serde_json::to_vec(&this.advertisement().beacon(false)) else {
                    continue;
                };
                let snapshot = locals.read().await.clone();
                if let Some(v6) = &v6 {
                    join_all_nodes(v6, &snapshot);
                }
                let destinations = match &targets {
                    Some(t) => t.clone(),
                    None => broadcast_targets(&snapshot),
                };
                for dest in destinations {
                    let socket = if dest.is_ipv4() {
                        Some(&v4)
                    } else {
                        v6.as_ref()
                    };
                    if let Some(socket) = socket {
                        if let Err(e) = socket.send_to(&bytes, dest).await {
                            debug!(%dest, error = %e, "beacon send failed");
                        }
                    }
                }
            }
        });
    }

    /// Handles one datagram. Returns a reply beacon to send back to `src` when
    /// this is a first-seen probe (D-F03-1).
    pub async fn handle_datagram(
        &self,
        bytes: &[u8],
        src: SocketAddr,
        locals: &[LocalAddr],
        now: Instant,
    ) -> Option<Beacon> {
        let beacon = match parse_beacon(bytes) {
            Ok(b) => b,
            Err(e) => {
                debug!(%src, error = %e, "dropping a malformed beacon");
                return None;
            }
        };
        self.handle_beacon(beacon, src, locals, now, DiscoveryMethod::LinkLocalProbe)
            .await
    }

    async fn handle_beacon(
        &self,
        beacon: Beacon,
        src: SocketAddr,
        locals: &[LocalAddr],
        now: Instant,
        via: DiscoveryMethod,
    ) -> Option<Beacon> {
        if *beacon.id == *self.own_id {
            return None;
        }
        if self.rate_limited(&src.ip().to_string(), now) {
            warn!(%src, "discovery rate limit exceeded; dropping beacon");
            return None;
        }
        let endpoint = endpoint_for(src, beacon.port, locals)?;

        let (event, reply) = {
            let mut peers = self.peers.write().await;
            if let Some(entry) = peers.get_mut(&beacon.id) {
                match entry.endpoints.iter().position(|e| e.addr == endpoint.addr) {
                    Some(i) => entry.endpoints[i] = endpoint,
                    None if entry.endpoints.len() < MAX_ENDPOINTS_PER_PEER => {
                        entry.endpoints.push(endpoint)
                    }
                    None => {}
                }
                entry
                    .endpoints
                    .sort_by_key(|e| std::cmp::Reverse(transport_rank(e.transport)));
                let updated = build_peer(&beacon, &entry.endpoints, entry.peer.via);
                entry.seen = now;
                let event =
                    (updated != entry.peer).then(|| DiscoveryEvent::Updated(updated.clone()));
                entry.peer = updated;
                let reply = !beacon.reply && !entry.replied;
                entry.replied |= reply;
                (event, reply)
            } else {
                if peers.len() >= MAX_PEERS {
                    warn!(%src, "peer table full; ignoring a new peer");
                    return None;
                }
                let endpoints = vec![endpoint];
                let peer = build_peer(&beacon, &endpoints, via);
                let reply = !beacon.reply;
                peers.insert(
                    beacon.id.clone(),
                    PeerEntry {
                        peer: peer.clone(),
                        seen: now,
                        endpoints,
                        replied: reply,
                    },
                );
                (Some(DiscoveryEvent::Discovered(peer)), reply)
            }
        };

        if let Some(event) = event {
            self.emit(event);
        }
        reply.then(|| self.advertisement().beacon(true))
    }

    /// Records a peer found by other means, applying the rate limiter.
    pub async fn observe_peer(&self, peer: Peer, source: &str) -> bool {
        let now = Instant::now();
        if self.rate_limited(source, now) {
            warn!(
                source,
                "discovery rate limit exceeded; dropping advertisement"
            );
            return false;
        }
        let event = {
            let mut peers = self.peers.write().await;
            match peers.get_mut(&peer.device_id) {
                Some(entry) => {
                    entry.seen = now;
                    let changed = entry.peer != peer;
                    entry.peer = peer.clone();
                    changed.then_some(DiscoveryEvent::Updated(peer))
                }
                None => {
                    if peers.len() >= MAX_PEERS {
                        return false;
                    }
                    peers.insert(
                        peer.device_id.clone(),
                        PeerEntry {
                            peer: peer.clone(),
                            seen: now,
                            endpoints: Vec::new(),
                            replied: true,
                        },
                    );
                    Some(DiscoveryEvent::Discovered(peer))
                }
            }
        };
        if let Some(event) = event {
            self.emit(event);
        }
        true
    }

    /// Drops peers unheard for longer than the TTL, emitting `Removed` for each.
    pub async fn expire(&self, now: Instant) {
        let removed: Vec<String> = {
            let mut peers = self.peers.write().await;
            let gone: Vec<String> = peers
                .iter()
                .filter(|(_, e)| now.saturating_duration_since(e.seen) > PEER_TTL)
                .map(|(id, _)| id.clone())
                .collect();
            for id in &gone {
                peers.remove(id);
            }
            gone
        };
        for device_id in removed {
            self.emit(DiscoveryEvent::Removed { device_id });
        }
    }

    /// Current peer list, best transport first, then by name so the order is
    /// stable across ticks. Expired entries are hidden even before the sweep.
    pub async fn peers(&self) -> Vec<Peer> {
        let now = Instant::now();
        let peers = self.peers.read().await;
        let mut list: Vec<Peer> = peers
            .values()
            .filter(|e| now.saturating_duration_since(e.seen) <= PEER_TTL)
            .map(|e| e.peer.clone())
            .collect();
        list.sort_by(|a, b| {
            transport_rank(b.transport)
                .cmp(&transport_rank(a.transport))
                .then_with(|| b.link.preference().cmp(&a.link.preference()))
                .then_with(|| a.display_name.cmp(&b.display_name))
        });
        list
    }

    /// Number of peers in the table, expired or not.
    pub async fn peer_count(&self) -> usize {
        self.peers.read().await.len()
    }

    /// Where a peer can be dialled, best transport first (for F02 fallback).
    pub async fn endpoints(&self, device_id: &str) -> Vec<Endpoint> {
        self.peers
            .read()
            .await
            .get(device_id)
            .map(|e| e.endpoints.clone())
            .unwrap_or_default()
    }

    pub async fn forget(&self, device_id: &str) {
        self.peers.write().await.remove(device_id);
    }

    /// Registers this device over mDNS and browses for peers (FR-DISC-002).
    ///
    /// `mdns-sd` runs its own thread; its events are read on a dedicated
    /// thread and fed through the same validation as a UDP beacon.
    fn start_mdns(&self, locals: Arc<RwLock<Vec<LocalAddr>>>) -> Result<()> {
        use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

        let mdns_error = |e: mdns_sd::Error| Error::Other(anyhow::anyhow!("mDNS: {e}"));
        let daemon = ServiceDaemon::new().map_err(mdns_error)?;
        let ad = self.advertisement();
        let props: HashMap<String, String> = ad.to_txt_pairs().into_iter().collect();
        let host = format!("{}.local.", ad.device_id);
        let info = ServiceInfo::new(SERVICE_TYPE, &ad.device_id, &host, (), ad.port, props)
            .map_err(mdns_error)?
            .enable_addr_auto();
        daemon.register(info).map_err(mdns_error)?;
        let events = daemon.browse(SERVICE_TYPE).map_err(mdns_error)?;

        let this = self.clone();
        let runtime = tokio::runtime::Handle::current();
        std::thread::Builder::new()
            .name("fasttransfer-mdns".into())
            .spawn(move || {
                // The daemon must outlive the browse; it lives with this thread.
                let _daemon = daemon;
                while let Ok(event) = events.recv() {
                    let ServiceEvent::ServiceResolved(info) = event else {
                        continue;
                    };
                    let Some(beacon) = beacon_from_txt(&info) else {
                        continue;
                    };
                    let snapshot = runtime.block_on(async { locals.read().await.clone() });
                    for ip in info.get_addresses() {
                        // An IPv6 link-local address from mDNS has no interface
                        // scope, so it cannot be dialled; beacons cover that
                        // case with a scoped address (D-F03-3).
                        if let IpAddr::V6(v6) = ip {
                            if is_ipv6_link_local(v6) {
                                continue;
                            }
                        }
                        let src = SocketAddr::new(*ip, beacon.port);
                        runtime.block_on(this.handle_beacon(
                            beacon.clone(),
                            src,
                            &snapshot,
                            Instant::now(),
                            DiscoveryMethod::Mdns,
                        ));
                    }
                }
            })?;
        Ok(())
    }
}

/// Builds a beacon from an mDNS TXT record and runs it through the same parser
/// as a UDP beacon, so both paths share one validation (§92).
fn beacon_from_txt(info: &mdns_sd::ServiceInfo) -> Option<Beacon> {
    let get = |k: &str| info.get_property_val_str(k).unwrap_or_default().to_string();
    let port = get("port").parse().unwrap_or_else(|_| info.get_port());
    let value = serde_json::json!({
        "v": BEACON_VERSION,
        "id": get("id"),
        "name": get("name"),
        "os": get("os"),
        "osv": get("osv"),
        "app": get("app"),
        "proto": get("proto"),
        "port": port,
        "tr": get("tr").split(',').filter(|s| !s.is_empty()).collect::<Vec<_>>(),
        "link": get("link"),
        // An mDNS sighting is never answered with a unicast beacon.
        "reply": true,
    });
    parse_beacon(&serde_json::to_vec(&value).ok()?).ok()
}

/// Whether this process should prefer one strategy over the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryMode {
    Auto,
    MdnsOnly,
    LinkLocalOnly,
}

impl DiscoveryMode {
    /// Reads `FASTTRANSFER_DISCOVERY`, defaulting to `auto`.
    pub fn from_env() -> Self {
        match std::env::var("FASTTRANSFER_DISCOVERY").as_deref() {
            Ok("mdns") => Self::MdnsOnly,
            Ok("linklocal") => Self::LinkLocalOnly,
            _ => Self::Auto,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn advert() -> Advertisement {
        Advertisement::new("dev-a".into(), "Laptop A".into(), "windows".into(), 47820)
    }

    #[test]
    fn txt_record_carries_no_file_information() {
        // SRS FR-DISC-009. If this test fails, the app is broadcasting
        // filesystem details to every device on the link.
        let pairs = advert().to_txt_pairs();
        let values: Vec<String> = pairs.iter().map(|(_, v)| v.to_lowercase()).collect();
        let joined = values.join(" ");

        for forbidden in ["file", "path", "dir", "download", "documents"] {
            assert!(
                !joined.contains(forbidden),
                "TXT record leaked {forbidden:?}: {joined}"
            );
        }
    }

    #[test]
    fn txt_record_includes_the_required_fields() {
        let pairs = advert().to_txt_pairs();
        let keys: Vec<&str> = pairs.iter().map(|(k, _)| k.as_str()).collect();
        // SRS FR-DISC-005
        for required in ["id", "name", "os", "app", "proto", "port"] {
            assert!(keys.contains(&required), "missing TXT key {required}");
        }
    }

    #[test]
    fn rate_limiter_allows_a_burst_then_blocks() {
        let mut limiter = RateLimiter::new(3, Duration::from_secs(1));
        let now = Instant::now();

        assert!(limiter.allow("peer-a", now));
        assert!(limiter.allow("peer-a", now));
        assert!(limiter.allow("peer-a", now));
        assert!(!limiter.allow("peer-a", now), "fourth should be blocked");
    }

    #[test]
    fn rate_limiter_is_per_source() {
        let mut limiter = RateLimiter::new(1, Duration::from_secs(1));
        let now = Instant::now();

        assert!(limiter.allow("peer-a", now));
        assert!(!limiter.allow("peer-a", now));
        assert!(limiter.allow("peer-b", now), "peer-b must not be affected");
    }

    #[test]
    fn rate_limiter_window_expires() {
        let mut limiter = RateLimiter::new(1, Duration::from_millis(50));
        let start = Instant::now();

        assert!(limiter.allow("peer-a", start));
        assert!(!limiter.allow("peer-a", start));
        assert!(limiter.allow("peer-a", start + Duration::from_millis(60)));
    }

    fn peer(id: &str, link: LinkKind) -> Peer {
        Peer {
            device_id: id.into(),
            display_name: id.into(),
            platform: "linux".into(),
            platform_version: String::new(),
            app_version: "0.1.0".into(),
            protocol_version: "1.0".into(),
            capabilities: Capabilities::default(),
            addresses: vec![],
            port: 47820,
            via: DiscoveryMethod::Mdns,
            link,
            transport: None,
            trusted: false,
        }
    }

    #[tokio::test]
    async fn peers_are_ordered_by_link_preference() {
        let discovery = Discovery::new(advert());
        discovery
            .observe_peer(peer("wifi-peer", LinkKind::ChargeOnly), "src")
            .await;
        discovery
            .observe_peer(peer("tb-peer", LinkKind::Thunderbolt), "src")
            .await;

        let list = discovery.peers().await;
        assert_eq!(list[0].device_id, "tb-peer");
    }

    #[tokio::test]
    async fn flood_from_one_source_is_dropped() {
        let discovery = Discovery::new(advert());
        for i in 0..100 {
            let mut p = peer(&format!("flood-{i}"), LinkKind::Thunderbolt);
            p.device_id = format!("flood-{i}");
            discovery.observe_peer(p, "attacker").await;
        }
        // The limiter allows 30/second; the rest must not land.
        assert!(discovery.peers().await.len() <= 30);
    }

    #[tokio::test]
    async fn forget_removes_a_peer() {
        let discovery = Discovery::new(advert());
        discovery
            .observe_peer(peer("gone", LinkKind::Thunderbolt), "src")
            .await;
        assert_eq!(discovery.peers().await.len(), 1);

        discovery.forget("gone").await;
        assert!(discovery.peers().await.is_empty());
    }

    // --- beacons (F03) -------------------------------------------------------

    use crate::link::InterfaceKind;
    use crate::transport::{LocalAddr, TransportKind};
    use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
    use std::sync::Mutex as StdMutex;

    fn named(id: &str, name: &str) -> Advertisement {
        Advertisement::new(id.into(), name.into(), "linux".into(), 47820)
    }

    fn beacon_bytes(id: &str, name: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "v": 1, "id": id, "name": name, "os": "linux", "osv": "", "app": "0.1.0",
            "proto": "1.0", "port": 47820, "tr": ["ethernet"], "link": "NONE", "reply": false
        }))
        .unwrap()
    }

    fn local(name: &str, ip: &str, index: u32, kind: InterfaceKind, virt: bool) -> LocalAddr {
        LocalAddr {
            name: name.into(),
            index: Some(index),
            ip: ip.parse().unwrap(),
            prefix_len: if ip.contains(':') { 64 } else { 16 },
            broadcast: None,
            kind,
            is_virtual: virt,
            up: true,
            link_speed_bps: None,
        }
    }

    fn capture(discovery: &Discovery) -> Arc<StdMutex<Vec<DiscoveryEvent>>> {
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let sink = seen.clone();
        discovery.set_event_sink(Arc::new(move |e| sink.lock().unwrap().push(e)));
        seen
    }

    /// AC-F03-02, extended by D-F03-4: transports and link capability are advertised.
    #[test]
    fn advertisement_carries_transports_and_link_capability() {
        let ad = named("dev-a", "Laptop A").with_link(
            vec!["thunderbolt".into(), "ethernet".into()],
            LinkKind::Thunderbolt,
        );
        let pairs = ad.to_txt_pairs();
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("tr").as_deref(), Some("thunderbolt,ethernet"));
        assert_eq!(get("link").as_deref(), Some("THUNDERBOLT"));

        let beacon = ad.beacon(false);
        assert_eq!(beacon.tr, vec!["thunderbolt", "ethernet"]);
        assert_eq!(beacon.link, "THUNDERBOLT");
        assert!(serde_json::to_vec(&beacon).unwrap().len() <= MAX_BEACON_BYTES);
    }

    /// AC-F03-05: malformed, oversized and lying beacons are refused.
    #[test]
    fn hostile_beacons_are_refused_by_the_parser() {
        assert!(parse_beacon(b"not json").is_err());
        assert!(parse_beacon(&vec![b' '; MAX_BEACON_BYTES + 1]).is_err());
        assert!(
            parse_beacon(&beacon_bytes("../etc", "x")).is_err(),
            "bad id"
        );
        assert!(
            parse_beacon(&beacon_bytes(&"a".repeat(65), "x")).is_err(),
            "long id"
        );

        let mut wrong_version: serde_json::Value =
            serde_json::from_slice(&beacon_bytes("dev-x", "x")).unwrap();
        wrong_version["v"] = 2.into();
        assert!(parse_beacon(&serde_json::to_vec(&wrong_version).unwrap()).is_err());

        let mut zero_port = wrong_version.clone();
        zero_port["v"] = 1.into();
        zero_port["port"] = 0.into();
        assert!(parse_beacon(&serde_json::to_vec(&zero_port).unwrap()).is_err());
    }

    /// AC-F03-05: untrusted display text is cleaned, never trusted.
    #[test]
    fn beacon_names_are_sanitised() {
        let nasty = format!("Evil\u{7}\nName\u{202e}{}", "x".repeat(500));
        let beacon = parse_beacon(&beacon_bytes("dev-x", &nasty)).unwrap();
        assert!(beacon.name.chars().count() <= crate::protocol::MAX_DISPLAY_NAME_CHARS);
        assert!(
            !beacon.name.chars().any(char::is_control),
            "{:?}",
            beacon.name
        );
        assert!(beacon.name.starts_with("EvilName"));

        let blank = parse_beacon(&beacon_bytes("dev-y", "\u{1}\u{2}  ")).unwrap();
        assert_eq!(blank.name, "Unknown device");
    }

    /// AC-F03-06: an IPv6 link-local endpoint keeps the receiving interface's scope.
    #[test]
    fn ipv6_link_local_endpoint_is_scoped() {
        let locals = vec![
            local("eth0", "fe80::1", 7, InterfaceKind::Ethernet, false),
            local("wlan0", "fe80::9", 3, InterfaceKind::Wifi, false),
        ];
        let src = SocketAddr::V6(SocketAddrV6::new("fe80::2".parse().unwrap(), 50000, 0, 7));

        let ep = endpoint_for(src, 47820, &locals).expect("endpoint");
        match ep.addr {
            SocketAddr::V6(v6) => {
                assert_eq!(*v6.ip(), "fe80::2".parse::<Ipv6Addr>().unwrap());
                assert_eq!(v6.scope_id(), 7, "scope id must be the receiving interface");
                assert_eq!(v6.port(), 47820, "connect to the advertised TCP port");
            }
            other => panic!("expected IPv6, got {other}"),
        }
        assert_eq!(ep.transport, Some(TransportKind::Ethernet));
        assert_eq!(ep.interface.as_deref(), Some("eth0"));
    }

    /// AC-F03-06: IPv4 endpoints are matched to the interface on the same subnet.
    #[test]
    fn ipv4_endpoint_is_matched_by_subnet() {
        let locals = vec![
            local(
                "Ethernet",
                "169.254.10.1",
                4,
                InterfaceKind::Ethernet,
                false,
            ),
            local("Wi-Fi", "192.168.1.20", 5, InterfaceKind::Wifi, false),
        ];
        let src: SocketAddr = "169.254.77.3:50000".parse().unwrap();
        let ep = endpoint_for(src, 47820, &locals).unwrap();
        assert_eq!(ep.addr, "169.254.77.3:47820".parse().unwrap());
        assert_eq!(ep.transport, Some(TransportKind::Ethernet));
    }

    /// FR-DISC-007: a beacon that arrived over a virtual adapter yields no endpoint.
    #[test]
    fn a_beacon_over_a_virtual_adapter_is_not_a_path() {
        let locals = vec![local(
            "vEthernet (WSL)",
            "172.20.0.1",
            9,
            InterfaceKind::Other,
            true,
        )];
        let src: SocketAddr = "172.20.0.5:50000".parse().unwrap();
        assert!(endpoint_for(src, 47820, &locals).is_none());
    }

    /// AC-F03-08: our own beacon never lists ourselves.
    #[tokio::test]
    async fn own_beacon_is_ignored() {
        let d = Discovery::new(named("dev-self", "Me"));
        let src: SocketAddr = "169.254.1.1:47820".parse().unwrap();
        let reply = d
            .handle_datagram(
                &beacon_bytes("dev-self", "Me"),
                src,
                &[],
                std::time::Instant::now(),
            )
            .await;
        assert!(reply.is_none());
        assert!(d.peers().await.is_empty());
    }

    /// AC-F03-09: a discovered peer is never trusted by discovery alone.
    #[tokio::test]
    async fn discovered_peers_are_untrusted() {
        let d = Discovery::new(named("dev-self", "Me"));
        let src: SocketAddr = "169.254.1.2:47820".parse().unwrap();
        d.handle_datagram(
            &beacon_bytes("dev-b", "Laptop B"),
            src,
            &[],
            std::time::Instant::now(),
        )
        .await;
        let peers = d.peers().await;
        assert_eq!(peers.len(), 1);
        assert!(!peers[0].trusted);
        assert_eq!(peers[0].via, DiscoveryMethod::LinkLocalProbe);
    }

    /// D-F03-1: a first-seen probe gets a unicast reply; a repeat does not.
    #[tokio::test]
    async fn a_new_peer_gets_one_reply() {
        let d = Discovery::new(named("dev-self", "Me"));
        let src: SocketAddr = "169.254.1.3:47820".parse().unwrap();
        let now = std::time::Instant::now();
        let first = d
            .handle_datagram(&beacon_bytes("dev-b", "B"), src, &[], now)
            .await;
        assert!(matches!(first, Some(ref b) if b.reply && b.id == "dev-self"));
        let again = d
            .handle_datagram(&beacon_bytes("dev-b", "B"), src, &[], now)
            .await;
        assert!(again.is_none());
    }

    /// FR-DISC-008, D-F03-5: discovered, updated and removed are all reported.
    #[tokio::test]
    async fn peer_changes_are_reported_as_events() {
        let d = Discovery::new(named("dev-self", "Me"));
        let seen = capture(&d);
        let src: SocketAddr = "169.254.1.4:47820".parse().unwrap();
        let now = std::time::Instant::now();

        d.handle_datagram(&beacon_bytes("dev-b", "Old name"), src, &[], now)
            .await;
        d.handle_datagram(&beacon_bytes("dev-b", "Old name"), src, &[], now)
            .await;
        d.handle_datagram(&beacon_bytes("dev-b", "New name"), src, &[], now)
            .await;

        let events = seen.lock().unwrap();
        assert_eq!(
            events.len(),
            2,
            "an unchanged repeat must not emit: {events:?}"
        );
        assert!(
            matches!(&events[0], DiscoveryEvent::Discovered(p) if p.display_name == "Old name")
        );
        assert!(matches!(&events[1], DiscoveryEvent::Updated(p) if p.display_name == "New name"));
    }

    /// AC-F03-07: a peer unheard for 30 s is removed with an event.
    #[tokio::test]
    async fn silent_peers_expire_with_an_event() {
        let d = Discovery::new(named("dev-self", "Me"));
        let seen = capture(&d);
        let src: SocketAddr = "169.254.1.5:47820".parse().unwrap();
        let t0 = std::time::Instant::now();
        d.handle_datagram(&beacon_bytes("dev-b", "B"), src, &[], t0)
            .await;

        d.expire(t0 + Duration::from_secs(29)).await;
        assert_eq!(d.peer_count().await, 1, "not yet expired");

        d.expire(t0 + Duration::from_secs(31)).await;
        assert_eq!(d.peer_count().await, 0);
        let events = seen.lock().unwrap();
        assert!(matches!(
            events.last(),
            Some(DiscoveryEvent::Removed { device_id }) if device_id == "dev-b"
        ));
    }

    /// Endpoints are offered best transport first, for the F02 fallback loop.
    #[tokio::test]
    async fn endpoints_are_ordered_by_transport() {
        let d = Discovery::new(named("dev-self", "Me"));
        let locals = vec![
            local("Wi-Fi", "192.168.1.20", 5, InterfaceKind::Wifi, false),
            local(
                "Ethernet",
                "169.254.10.1",
                4,
                InterfaceKind::Ethernet,
                false,
            ),
        ];
        let now = std::time::Instant::now();
        let wifi_src: SocketAddr = "192.168.1.30:47820".parse().unwrap();
        let eth_src: SocketAddr = "169.254.10.2:47820".parse().unwrap();
        d.handle_datagram(&beacon_bytes("dev-b", "B"), wifi_src, &locals, now)
            .await;
        d.handle_datagram(&beacon_bytes("dev-b", "B"), eth_src, &locals, now)
            .await;

        let eps = d.endpoints("dev-b").await;
        assert_eq!(eps.len(), 2);
        assert_eq!(eps[0].transport, Some(TransportKind::Ethernet));
        assert_eq!(eps[1].transport, Some(TransportKind::Wifi));
        assert_eq!(d.peers().await[0].transport, Some(TransportKind::Ethernet));
    }

    /// AC-F03-04: two instances find each other over UDP with mDNS off.
    #[tokio::test]
    async fn beacons_find_a_peer_without_mdns() {
        let b = Discovery::new(named("dev-b", "Laptop B"));
        let b_addr = b
            .start_with(DiscoveryConfig::for_test(vec![]))
            .await
            .unwrap();
        let a = Discovery::new(named("dev-a", "Laptop A"));
        a.start_with(DiscoveryConfig::for_test(vec![b_addr]))
            .await
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let a_sees_b = a.peers().await.iter().any(|p| p.device_id == "dev-b");
            let b_sees_a = b.peers().await.iter().any(|p| p.device_id == "dev-a");
            if a_sees_b && b_sees_a {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "peers did not find each other in 5 s"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        // The stored endpoint is the packet's source, on the advertised TCP port.
        let eps = a.endpoints("dev-b").await;
        assert_eq!(eps[0].addr.ip(), b_addr.ip());
        assert_eq!(eps[0].addr.port(), 47820);
    }

    /// AC-F03-05, live: garbage on the socket neither crashes nor pollutes the list.
    #[tokio::test]
    async fn hostile_datagrams_do_not_reach_the_peer_list() {
        let d = Discovery::new(named("dev-self", "Me"));
        let addr = d
            .start_with(DiscoveryConfig::for_test(vec![]))
            .await
            .unwrap();
        let sock = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();

        sock.send_to(b"\xff\xfe garbage", addr).await.unwrap();
        sock.send_to(&vec![b'{'; 4000], addr).await.unwrap();
        sock.send_to(&beacon_bytes("../../x", "x"), addr)
            .await
            .unwrap();
        sock.send_to(&beacon_bytes("dev-ok", "Fine\u{0}"), addr)
            .await
            .unwrap();

        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while d.peer_count().await == 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "valid beacon never landed"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let peers = d.peers().await;
        assert_eq!(peers.len(), 1);
        assert_eq!(peers[0].device_id, "dev-ok");
        assert_eq!(peers[0].display_name, "Fine");
    }
}
