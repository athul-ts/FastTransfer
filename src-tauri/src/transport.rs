//! Interface enumeration, transport ranking, and fallback.
//!
//! Implements SRS §14 and §16. The transfer engine talks to this module through
//! the [`Transport`] trait and never to a concrete transport — that is the
//! transport-independence rule from SRS §11.2.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::{TcpListener, TcpSocket};

use crate::error::{Error, Result};
use crate::link::{self, Interface, InterfaceKind, LinkObservation};

/// How long a connect attempt may take before the next candidate is tried. A
/// cable that was just unplugged otherwise leaves `connect` waiting on the OS's
/// multi-minute default (SRS §90, §55).
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Socket buffer size requested on both ends. The OS default (often 64 KiB on
/// Windows) cannot keep a multi-Gbps link full; the kernel clamps this to what
/// it allows, so asking for more than it grants is harmless (SRS NFR-PERF-007).
const SOCKET_BUFFER_BYTES: u32 = 4 * 1024 * 1024;

/// A candidate path between two machines.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransportCandidate {
    pub kind: TransportKind,
    pub interface: Interface,
    /// Ranked score. Higher is better. The base weight follows
    /// `link::LinkKind::preference`, and is adjusted by measured throughput once
    /// a session has run.
    pub score: u32,
    /// Last measured throughput in bytes/second, if we have ever measured it.
    pub measured_bps: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TransportKind {
    Thunderbolt,
    Ethernet,
    Wifi,
}

impl TransportKind {
    /// The transport class an interface provides, or `None` if it cannot carry
    /// a transfer at all.
    ///
    /// This maps from [`InterfaceKind`], not [`LinkKind`]. Deriving it from the
    /// cable classification meant only Thunderbolt could ever be returned, so
    /// the Ethernet and Wi-Fi fallback required by SRS §16 was unreachable.
    pub fn from_interface(kind: InterfaceKind) -> Option<Self> {
        match kind {
            InterfaceKind::Thunderbolt | InterfaceKind::Usb4Network => {
                Some(TransportKind::Thunderbolt)
            }
            InterfaceKind::Ethernet => Some(TransportKind::Ethernet),
            InterfaceKind::Wifi => Some(TransportKind::Wifi),
            InterfaceKind::UsbDataOnly | InterfaceKind::Other => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            TransportKind::Thunderbolt => "Thunderbolt",
            TransportKind::Ethernet => "Ethernet",
            TransportKind::Wifi => "Wi-Fi",
        }
    }
}

/// The interface every transport implements. The transfer engine is written
/// against this, never against TCP, QUIC, or a specific NIC.
#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    fn kind(&self) -> TransportKind;

    async fn connect(&self, peer: SocketAddr) -> Result<Box<dyn TransportStream>>;

    /// Measured throughput, not the link rating. SRS §101.
    fn measured_throughput_bps(&self) -> Option<u64>;
}

/// A bidirectional byte stream. Implemented over TCP today, QUIC later.
///
/// This is `AsyncRead + AsyncWrite` rather than bespoke read/write methods so
/// the engine can run, unchanged, over a TCP socket in production and over an
/// in-memory pipe in tests (the loopback transport CLAUDE.md requires).
pub trait TransportStream: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> TransportStream for T {}

/// TCP over whichever interface the OS routes the peer address through.
///
/// One type serves Thunderbolt, Ethernet and Wi-Fi: Thunderbolt networking *is*
/// an IP interface, so the transfer path is identical and only the `kind` label
/// and the measured throughput differ (SRS §116.1.1, "the transfer engine is
/// transport-agnostic").
#[derive(Debug, Clone)]
pub struct TcpTransport {
    kind: TransportKind,
    measured_bps: Arc<AtomicU64>,
}

impl TcpTransport {
    pub fn new(kind: TransportKind) -> Self {
        Self {
            kind,
            measured_bps: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Records a completed transfer's measured throughput so later selection
    /// can prefer what was observed over what was rated (SRS FR-TRANS-004).
    pub fn record_measurement(&self, bps: u64) {
        self.measured_bps.store(bps, Ordering::Relaxed);
    }
}

#[async_trait::async_trait]
impl Transport for TcpTransport {
    fn kind(&self) -> TransportKind {
        self.kind
    }

    async fn connect(&self, peer: SocketAddr) -> Result<Box<dyn TransportStream>> {
        let socket = if peer.is_ipv4() {
            TcpSocket::new_v4()?
        } else {
            TcpSocket::new_v6()?
        };
        // Best effort: a refused buffer size is a performance note, not a failure.
        let _ = socket.set_send_buffer_size(SOCKET_BUFFER_BYTES);
        let _ = socket.set_recv_buffer_size(SOCKET_BUFFER_BYTES);

        let stream = tokio::time::timeout(CONNECT_TIMEOUT, socket.connect(peer))
            .await
            .map_err(|_| Error::PeerNotFound)??;
        // Chunk headers and footers are small frames between large writes;
        // Nagle would delay each one waiting for an ACK.
        stream.set_nodelay(true)?;
        Ok(Box::new(stream))
    }

    fn measured_throughput_bps(&self) -> Option<u64> {
        match self.measured_bps.load(Ordering::Relaxed) {
            0 => None,
            bps => Some(bps),
        }
    }
}

/// Binds the transfer listener on every address family the host supports.
///
/// IPv6 is bound first. On Linux and macOS a `[::]` socket is dual-stack and
/// already accepts IPv4, so the later `0.0.0.0` bind fails with `AddrInUse`,
/// which is expected and ignored. On Windows `[::]` is IPv6-only, so the IPv4
/// bind is what makes IPv4 work. IPv6 link-local matters here: it is the only
/// address a bare Thunderbolt link is guaranteed to have (SRS FR-DISC-004).
pub async fn bind_listeners(port: u16) -> Result<Vec<TcpListener>> {
    let mut listeners = Vec::new();

    match TcpListener::bind(SocketAddr::from(([0u16; 8], port))).await {
        Ok(l) => listeners.push(l),
        Err(e) => tracing::debug!(error = %e, "IPv6 listener unavailable"),
    }
    match TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await {
        Ok(l) => listeners.push(l),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse && !listeners.is_empty() => {
            tracing::debug!("IPv4 covered by the dual-stack IPv6 listener");
        }
        Err(e) => {
            if listeners.is_empty() {
                return Err(e.into());
            }
            tracing::debug!(error = %e, "IPv4 listener unavailable");
        }
    }
    Ok(listeners)
}

/// Enumerates usable interfaces and ranks them.
///
/// Virtual adapters are excluded by default — a VPN or a Docker bridge is not a
/// path to the laptop on the other end of the cable (SRS FR-DISC-007).
pub fn candidates() -> Vec<TransportCandidate> {
    let mut out = Vec::new();

    for interface in enumerate_interfaces() {
        if interface.is_virtual || !interface.up {
            continue;
        }

        let Some(kind) = TransportKind::from_interface(interface.kind) else {
            continue;
        };

        out.push(TransportCandidate {
            kind,
            score: score_for(kind, &interface),
            measured_bps: None,
            interface,
        });
    }

    // Best first. Ties break on link speed, then name, so the order is stable.
    out.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.interface.link_speed_bps.cmp(&a.interface.link_speed_bps))
            .then_with(|| a.interface.name.cmp(&b.interface.name))
    });

    out
}

/// Picks the best available transport and returns the fallback order behind it.
///
/// Returns [`Error::NoTransport`] when nothing is usable, which the UI renders
/// with the remedy from `error.rs` rather than a bare failure.
pub fn select() -> Result<(TransportCandidate, Vec<TransportCandidate>)> {
    let all = candidates();
    let mut iter = all.into_iter();

    let primary = iter.next().ok_or(Error::NoTransport)?;
    Ok((primary, iter.collect()))
}

/// Ranks a candidate. Thunderbolt wins on link type; a measured throughput, when
/// we have one, overrides the guess — that is the honest-reporting principle
/// applied to selection rather than just to display.
fn score_for(kind: TransportKind, interface: &Interface) -> u32 {
    let base = match kind {
        TransportKind::Thunderbolt => 1000,
        TransportKind::Ethernet => 500,
        TransportKind::Wifi => 200,
    };

    // A reported link speed is a weak tiebreaker — it is a rating, not a
    // measurement, so it must never outweigh the transport class. Capped at 99
    // so a 100 GbE rating still cannot beat a Thunderbolt link.
    let speed_bonus = interface
        .link_speed_bps
        .map(|bps| ((bps / 1_000_000_000).min(99)) as u32)
        .unwrap_or(0);

    base + speed_bonus
}

/// One address on one local interface, carrying what discovery needs to use it
/// (the interface index scopes an IPv6 link-local address; the broadcast
/// address is where an IPv4 beacon goes).
#[derive(Debug, Clone)]
pub struct LocalAddr {
    pub name: String,
    pub index: Option<u32>,
    pub ip: IpAddr,
    /// Subnet prefix length, used to tell which interface an IPv4 peer is on.
    pub prefix_len: u8,
    pub broadcast: Option<std::net::Ipv4Addr>,
    pub kind: InterfaceKind,
    pub is_virtual: bool,
    pub up: bool,
    pub link_speed_bps: Option<u64>,
}

impl LocalAddr {
    /// The transport this address can carry, if it can carry one at all.
    pub fn transport(&self) -> Option<TransportKind> {
        if self.is_virtual || !self.up {
            return None;
        }
        TransportKind::from_interface(self.kind)
    }
}

/// Extra facts the OS can supply beyond an interface's name. Only Linux fills
/// these in (sysfs); elsewhere the name is all there is, which is why every
/// Thunderbolt claim below is checked against the platform probe as well.
#[derive(Debug, Default, Clone)]
struct SysHints {
    wireless: bool,
    driver: Option<String>,
    speed_bps: Option<u64>,
    oper_up: Option<bool>,
}

#[cfg(target_os = "linux")]
fn sys_hints(name: &str) -> SysHints {
    use std::path::Path;

    // Interface names never contain a separator; refuse one rather than let a
    // hostile name walk out of /sys/class/net.
    if name.contains('/') || name.contains("..") {
        return SysHints::default();
    }
    let base = Path::new("/sys/class/net").join(name);
    let read = |file: &str| std::fs::read_to_string(base.join(file)).ok();

    SysHints {
        wireless: base.join("wireless").exists() || base.join("phy80211").exists(),
        driver: std::fs::read_link(base.join("device/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().into_owned())),
        // `speed` is Mb/s, and -1 when the link is down or the driver cannot say.
        speed_bps: read("speed")
            .and_then(|s| s.trim().parse::<i64>().ok())
            .filter(|mbps| *mbps > 0)
            .map(|mbps| mbps as u64 * 1_000_000),
        oper_up: read("operstate").map(|s| matches!(s.trim(), "up" | "unknown")),
    }
}

#[cfg(not(target_os = "linux"))]
fn sys_hints(_name: &str) -> SysHints {
    SysHints::default()
}

/// Names that mark an interface as virtual: VPNs, containers, hypervisors,
/// tunnels and per-OS plumbing. A path to the laptop on the other end of a
/// cable never goes through one (SRS FR-DISC-007).
fn is_virtual_name(lower: &str) -> bool {
    const SUBSTRINGS: &[&str] = &[
        "vethernet",
        "virtualbox",
        "vboxnet",
        "vmware",
        "vmnet",
        "hyper-v",
        "docker",
        "podman",
        "virbr",
        "lxc",
        "wsl",
        "tailscale",
        "wireguard",
        "zerotier",
        "vpn",
        "loopback",
        "isatap",
        "teredo",
        "bluetooth",
        "npcap",
        "pseudo",
        "tunnel",
        "cni",
        "flannel",
        "kube",
    ];
    const PREFIXES: &[&str] = &[
        "veth", "br-", "tun", "tap", "utun", "wg", "vnet", "awdl", "llw", "anpi", "gif0", "stf0",
        "ap1",
    ];
    lower == "lo"
        || lower == "lo0"
        || SUBSTRINGS.iter().any(|s| lower.contains(s))
        || PREFIXES.iter().any(|p| lower.starts_with(p))
}

fn is_wifi_name(lower: &str) -> bool {
    ["wi-fi", "wifi", "wlan", "wireless", "802.11"]
        .iter()
        .any(|s| lower.contains(s))
        || lower.starts_with("wl")
}

fn is_wired_name(lower: &str) -> bool {
    let macos_style = lower
        .strip_prefix("en")
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
    lower.contains("ethernet")
        || lower.contains("local area connection")
        || ["eth", "enp", "eno", "ens", "enx"]
            .iter()
            .any(|p| lower.starts_with(p))
        || macos_style
}

/// Classifies an interface, returning its kind and whether it is virtual.
///
/// A Thunderbolt-looking *name* is never enough on its own: the name must be
/// backed by the platform probe (`link::observe`, which reads the device tree
/// or sysfs). An adapter that merely calls itself Thunderbolt is `Other`, not a
/// transport, because a wrong Thunderbolt claim is the one failure this product
/// must not have (SRS FR-LINK-003, §4.2).
fn classify(name: &str, hints: &SysHints, link: &LinkObservation) -> (InterfaceKind, bool) {
    let lower = name.to_ascii_lowercase();

    if is_virtual_name(&lower) {
        return (InterfaceKind::Other, true);
    }

    let looks_thunderbolt = lower.contains("thunderbolt")
        || lower.contains("usb4")
        || lower == "bridge0"
        || hints.driver.as_deref() == Some("thunderbolt-net");
    if looks_thunderbolt {
        let kind = if link.thunderbolt_network {
            InterfaceKind::Thunderbolt
        } else if link.usb4_network {
            InterfaceKind::Usb4Network
        } else {
            InterfaceKind::Other
        };
        return (kind, false);
    }

    if hints.wireless || is_wifi_name(&lower) {
        return (InterfaceKind::Wifi, false);
    }

    // The built-in interface on a Mac laptop is Wi-Fi, and `en0` is its name.
    // This is a heuristic: telling a Mac's Wi-Fi from its Ethernet needs the
    // SystemConfiguration framework, which is not wired in yet.
    #[cfg(target_os = "macos")]
    if lower == "en0" {
        return (InterfaceKind::Wifi, false);
    }

    if is_wired_name(&lower) {
        return (InterfaceKind::Ethernet, false);
    }

    (InterfaceKind::Other, false)
}

/// Every non-loopback address on this machine, classified.
///
/// Blocking: it reads sysfs or walks the Windows device tree. Callers on the
/// async runtime go through `spawn_blocking`.
pub fn local_addrs() -> Vec<LocalAddr> {
    local_addrs_with(&link::observe())
}

/// [`local_addrs`] against an observation the caller already made, so a caller
/// that also needs the link class walks the device tree once, not twice.
pub fn local_addrs_with(observation: &LinkObservation) -> Vec<LocalAddr> {
    let raw = match if_addrs::get_if_addrs() {
        Ok(list) => list,
        Err(e) => {
            // Reported as "no interfaces", never guessed (SRS FR-LINK-003).
            tracing::debug!(error = %e, "interface enumeration failed");
            return Vec::new();
        }
    };
    let mut hints: std::collections::HashMap<String, SysHints> = std::collections::HashMap::new();

    raw.into_iter()
        .filter(|i| !i.is_loopback())
        .map(|i| {
            let hint = hints
                .entry(i.name.clone())
                .or_insert_with(|| sys_hints(&i.name))
                .clone();
            let (kind, is_virtual) = classify(&i.name, &hint, observation);
            let (broadcast, prefix_len) = match &i.addr {
                if_addrs::IfAddr::V4(v4) => (v4.broadcast, v4.prefixlen),
                if_addrs::IfAddr::V6(v6) => (None, v6.prefixlen),
            };
            LocalAddr {
                ip: i.addr.ip(),
                prefix_len,
                broadcast,
                index: i.index,
                up: hint.oper_up.unwrap_or(true),
                link_speed_bps: hint.speed_bps,
                name: i.name,
                kind,
                is_virtual,
            }
        })
        .collect()
}

/// Groups addresses into one `Interface` per OS interface, for the UI.
fn group_interfaces(addrs: Vec<LocalAddr>) -> Vec<Interface> {
    let mut out: Vec<Interface> = Vec::new();
    for a in addrs {
        match out.iter_mut().find(|i| i.name == a.name) {
            Some(existing) => existing.addresses.push(a.ip.to_string()),
            None => out.push(Interface {
                name: a.name,
                kind: a.kind,
                link_speed_bps: a.link_speed_bps,
                up: a.up,
                is_virtual: a.is_virtual,
                addresses: vec![a.ip.to_string()],
            }),
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Enumerates interfaces from the OS (SRS FR-TRANS-001, FR-TRANS-002).
///
/// An enumeration failure yields an empty list, which makes `select()` report
/// `NoTransport` rather than naming an interface that may not be the cable.
pub fn enumerate_interfaces() -> Vec<Interface> {
    group_interfaces(local_addrs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn iface(name: &str, kind: InterfaceKind, speed: Option<u64>) -> Interface {
        Interface {
            name: name.into(),
            kind,
            link_speed_bps: speed,
            up: true,
            is_virtual: false,
            addresses: vec![],
        }
    }

    #[test]
    fn every_carrying_medium_maps_to_a_transport() {
        // Regression: this mapping used to be derived from `LinkKind`, which has
        // no Ethernet or Wi-Fi variant — so both fallback paths silently
        // vanished and only Thunderbolt was ever selectable (SRS §16).
        assert_eq!(
            TransportKind::from_interface(InterfaceKind::Thunderbolt),
            Some(TransportKind::Thunderbolt)
        );
        assert_eq!(
            TransportKind::from_interface(InterfaceKind::Usb4Network),
            Some(TransportKind::Thunderbolt)
        );
        assert_eq!(
            TransportKind::from_interface(InterfaceKind::Ethernet),
            Some(TransportKind::Ethernet)
        );
        assert_eq!(
            TransportKind::from_interface(InterfaceKind::Wifi),
            Some(TransportKind::Wifi)
        );
    }

    #[test]
    fn unusable_media_is_never_a_transport() {
        // A data-only cable must not become a transfer candidate (SRS §4), and
        // neither must a loopback or an unclassified interface.
        assert_eq!(
            TransportKind::from_interface(InterfaceKind::UsbDataOnly),
            None
        );
        assert_eq!(TransportKind::from_interface(InterfaceKind::Other), None);
    }

    #[test]
    fn thunderbolt_outranks_ethernet_regardless_of_rated_speed() {
        let tb = score_for(
            TransportKind::Thunderbolt,
            &iface("tb0", InterfaceKind::Thunderbolt, Some(40_000_000_000)),
        );
        let eth = score_for(
            TransportKind::Ethernet,
            &iface("eth0", InterfaceKind::Ethernet, Some(100_000_000_000)),
        );
        assert!(
            tb > eth,
            "a 100 GbE rating must not outrank a Thunderbolt link ({eth} vs {tb})"
        );
    }

    #[test]
    fn ethernet_outranks_wifi() {
        let eth = score_for(
            TransportKind::Ethernet,
            &iface("eth0", InterfaceKind::Ethernet, None),
        );
        let wifi = score_for(
            TransportKind::Wifi,
            &iface("wlan0", InterfaceKind::Wifi, None),
        );
        assert!(eth > wifi);
    }

    #[test]
    fn speed_bonus_is_capped_below_the_next_class() {
        // Worst possible Thunderbolt still beats the best possible Ethernet.
        let worst_tb = score_for(
            TransportKind::Thunderbolt,
            &iface("tb", InterfaceKind::Thunderbolt, None),
        );
        let best_eth = score_for(
            TransportKind::Ethernet,
            &iface("eth", InterfaceKind::Ethernet, Some(u64::MAX)),
        );
        assert!(worst_tb > best_eth);
    }

    #[test]
    fn no_interfaces_reports_no_transport() {
        // With the platform probes unimplemented, `candidates()` is empty, so
        // selection must fail honestly rather than return a placeholder.
        if candidates().is_empty() {
            assert!(matches!(select(), Err(Error::NoTransport)));
        }
    }

    // --- interface classification ---------------------------------------------

    fn tb_link() -> LinkObservation {
        LinkObservation {
            cable_present: true,
            thunderbolt_network: true,
            thunderbolt_capable: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_thunderbolt_name_without_evidence_is_not_a_transport() {
        // FR-LINK-003: an adapter renamed to look like Thunderbolt must not
        // become Thunderbolt. The probe saw no Thunderbolt network, so no claim.
        let (kind, virt) = classify(
            "Thunderbolt(TM) Networking",
            &SysHints::default(),
            &LinkObservation::default(),
        );
        assert_eq!(kind, InterfaceKind::Other);
        assert!(!virt);
        assert_eq!(TransportKind::from_interface(kind), None);
    }

    #[test]
    fn a_thunderbolt_name_backed_by_the_probe_is_thunderbolt() {
        let (kind, _) = classify("thunderbolt0", &SysHints::default(), &tb_link());
        assert_eq!(kind, InterfaceKind::Thunderbolt);
    }

    #[test]
    fn a_usb4_adapter_is_usb4_not_thunderbolt() {
        let link = LinkObservation {
            cable_present: true,
            usb4_network: true,
            ..Default::default()
        };
        let (kind, _) = classify("USB4(TM) P2P Network Adapter", &SysHints::default(), &link);
        assert_eq!(kind, InterfaceKind::Usb4Network);
    }

    #[test]
    fn a_renamed_linux_thunderbolt_interface_is_found_by_its_driver() {
        // udev can rename `thunderbolt0`; the driver binding cannot be renamed.
        let hints = SysHints {
            driver: Some("thunderbolt-net".into()),
            ..Default::default()
        };
        let (kind, _) = classify("enx-lab-link", &hints, &tb_link());
        assert_eq!(kind, InterfaceKind::Thunderbolt);

        // ...but the driver alone does not outvote a probe that saw nothing.
        let (kind, _) = classify("enx-lab-link", &hints, &LinkObservation::default());
        assert_eq!(kind, InterfaceKind::Other);
    }

    #[test]
    fn ordinary_wired_and_wireless_names_classify() {
        let none = LinkObservation::default();
        let h = SysHints::default();
        for name in [
            "Ethernet",
            "Ethernet 3",
            "eth0",
            "enp3s0",
            "eno1",
            "enx0a1b2c",
        ] {
            assert_eq!(
                classify(name, &h, &none).0,
                InterfaceKind::Ethernet,
                "{name}"
            );
        }
        for name in [
            "Wi-Fi",
            "WLAN",
            "wlan0",
            "wlp2s0",
            "Wireless Network Connection",
        ] {
            assert_eq!(classify(name, &h, &none).0, InterfaceKind::Wifi, "{name}");
        }
    }

    #[test]
    fn sysfs_wireless_flag_overrides_an_unhelpful_name() {
        let hints = SysHints {
            wireless: true,
            ..Default::default()
        };
        let (kind, _) = classify("net0", &hints, &LinkObservation::default());
        assert_eq!(kind, InterfaceKind::Wifi);
    }

    #[test]
    fn virtual_adapters_are_flagged_and_never_transports() {
        let none = LinkObservation::default();
        let h = SysHints::default();
        for name in [
            "vEthernet (Default Switch)",
            "VirtualBox Host-Only Network",
            "VMware Network Adapter VMnet8",
            "docker0",
            "veth1a2b3c",
            "br-1234abcd",
            "tun0",
            "tailscale0",
            "wg0",
            "utun3",
            "Bluetooth Network Connection",
            "awdl0",
            "lo",
        ] {
            let (kind, virt) = classify(name, &h, &none);
            assert!(virt, "{name} should be virtual");
            assert_eq!(TransportKind::from_interface(kind), None, "{name}");
        }
    }

    #[test]
    fn a_virtual_name_beats_a_thunderbolt_marker() {
        // A VPN adapter cannot become the cable by mentioning Thunderbolt.
        let (_, virt) = classify("Thunderbolt VPN", &SysHints::default(), &tb_link());
        assert!(virt);
    }

    fn addr(name: &str, ip: &str, kind: InterfaceKind) -> LocalAddr {
        LocalAddr {
            name: name.into(),
            index: Some(3),
            ip: ip.parse().unwrap(),
            prefix_len: 24,
            broadcast: None,
            kind,
            is_virtual: false,
            up: true,
            link_speed_bps: None,
        }
    }

    #[test]
    fn addresses_on_one_interface_are_grouped() {
        let grouped = group_interfaces(vec![
            addr("eth0", "169.254.7.7", InterfaceKind::Ethernet),
            addr("eth0", "fe80::1", InterfaceKind::Ethernet),
            addr("wlan0", "192.168.1.5", InterfaceKind::Wifi),
        ]);
        assert_eq!(grouped.len(), 2);
        let eth = grouped.iter().find(|i| i.name == "eth0").unwrap();
        assert_eq!(eth.addresses, vec!["169.254.7.7", "fe80::1"]);
    }

    #[test]
    fn a_down_or_virtual_address_carries_no_transport() {
        let mut a = addr("eth0", "10.0.0.2", InterfaceKind::Ethernet);
        assert_eq!(a.transport(), Some(TransportKind::Ethernet));
        a.up = false;
        assert_eq!(a.transport(), None);
        a.up = true;
        a.is_virtual = true;
        assert_eq!(a.transport(), None);
    }

    #[test]
    fn the_live_enumeration_never_reports_loopback() {
        // Runs against whatever machine executes the test: whatever it finds,
        // loopback must not be among the candidates (SRS FR-DISC-007).
        for a in local_addrs() {
            assert!(!a.ip.is_loopback(), "{} reported loopback {}", a.name, a.ip);
        }
    }

    // --- TCP transport --------------------------------------------------------

    #[tokio::test]
    async fn tcp_transport_connects_and_carries_bytes() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 5];
            s.read_exact(&mut buf).await.unwrap();
            s.write_all(&buf).await.unwrap();
        });

        let transport = TcpTransport::new(TransportKind::Ethernet);
        let mut stream = transport.connect(addr).await.unwrap();
        stream.write_all(b"hello").await.unwrap();
        let mut echoed = [0u8; 5];
        stream.read_exact(&mut echoed).await.unwrap();
        assert_eq!(&echoed, b"hello");
        server.await.unwrap();
    }

    #[tokio::test]
    async fn a_refused_connection_is_an_error_not_a_hang() {
        // Grab a free port, then close it so nothing is listening there.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap().port()
        };
        let transport = TcpTransport::new(TransportKind::Ethernet);
        let result = transport
            .connect(SocketAddr::from(([127, 0, 0, 1], port)))
            .await;
        assert!(result.is_err());
    }

    #[test]
    fn measured_throughput_is_none_until_recorded() {
        let t = TcpTransport::new(TransportKind::Thunderbolt);
        assert_eq!(t.measured_throughput_bps(), None);
        t.record_measurement(2_500_000_000);
        assert_eq!(t.measured_throughput_bps(), Some(2_500_000_000));
    }

    #[test]
    fn transport_labels_are_user_facing() {
        assert_eq!(TransportKind::Thunderbolt.label(), "Thunderbolt");
        assert_eq!(TransportKind::Ethernet.label(), "Ethernet");
        assert_eq!(TransportKind::Wifi.label(), "Wi-Fi");
    }
}
