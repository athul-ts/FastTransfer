//! Peer discovery, with a router-free fallback.
//!
//! Implements SRS §13. The requirement that shapes this module is FR-DISC-003/004:
//! **discovery must work on a direct cable with no router and no DHCP server.**
//!
//! That rules out relying on mDNS alone. On a bare cable link there is usually no
//! mDNS responder, and the two machines have link-local addresses
//! (`169.254.0.0/16`, `fe80::/10`) that neither side has advertised. So the
//! module runs two strategies and merges their results:
//!
//! | Strategy | Works when | Fails when |
//! |---|---|---|
//! | mDNS / DNS-SD | both hosts have normal multicast networking | no responder, or multicast filtered |
//! | Link-local probe | there is a direct L2 path | the interface is not link-local |
//!
//! The advertised TXT record carries what FR-DISC-005 requires — identity and
//! capability — and nothing FR-DISC-009 forbids. Never a filename, never a path.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::link::LinkKind;
use crate::protocol::{Capabilities, PROTOCOL_VERSION};

/// The DNS-SD service type. `_fasttransfer._tcp.local.`
pub const SERVICE_TYPE: &str = "_fasttransfer._tcp.local.";

/// Default port for discovery and for the transfer listener.
pub const DEFAULT_PORT: u16 = 47820;

/// How often to re-advertise over mDNS.
///
/// Unused until the Phase 3 mDNS loop lands (SRS §108). Recorded now because
/// the cadence is a design decision: re-advertising every 5s bounds how long a
/// newly connected peer stays invisible.
#[allow(dead_code)]
const ADVERTISE_INTERVAL: Duration = Duration::from_secs(5);

/// How long a peer may go unheard before it is dropped from the list.
const PEER_TTL: Duration = Duration::from_secs(30);

/// Link-local probe cadence.
///
/// Unused until `probe_link_local` is implemented (SRS §108). 3s is fast enough
/// that a bare cable feels instant, and slow enough that probing a /16 does not
/// become the flood FR-DISC-010 exists to prevent.
#[allow(dead_code)]
const PROBE_INTERVAL: Duration = Duration::from_secs(3);

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
    pub capabilities: Capabilities,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    /// How we found it. Shown in diagnostics so a failure is explainable.
    pub via: DiscoveryMethod,
    pub link: LinkKind,
    pub trusted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiscoveryMethod {
    /// Found via mDNS/DNS-SD.
    Mdns,
    /// Found by probing the link-local subnet.
    LinkLocalProbe,
}

/// This device's own advertisement.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
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
        }
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
        ]
    }
}

/// Rate limiter for inbound discovery traffic (SRS FR-DISC-010, §93).
///
/// A peer that floods the link must not be able to saturate the CPU or the UI.
struct RateLimiter {
    per_source: HashMap<String, Vec<std::time::Instant>>,
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

    fn allow(&mut self, source: &str, now: std::time::Instant) -> bool {
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

/// The discovery service.
///
/// Cheap to clone, and every clone shares one peer table and one rate limiter.
/// That sharing is the point: the background advertising task and the IPC layer
/// must see the same device list. A clone with its own peer map would report an
/// empty device list while discovery was working perfectly.
#[derive(Clone)]
pub struct Discovery {
    advertisement: Arc<Advertisement>,
    peers: Arc<RwLock<HashMap<String, (Peer, std::time::Instant)>>>,
    /// `Mutex`, not `RwLock`: every access is a short synchronous update, and
    /// the guard is never held across an `await`.
    limiter: Arc<Mutex<RateLimiter>>,
}

impl Discovery {
    pub fn new(advertisement: Advertisement) -> Self {
        Self {
            advertisement: Arc::new(advertisement),
            peers: Arc::new(RwLock::new(HashMap::new())),
            limiter: Arc::new(Mutex::new(RateLimiter::new(30, Duration::from_secs(1)))),
        }
    }

    pub fn advertisement(&self) -> &Advertisement {
        &self.advertisement
    }

    /// Starts advertising this device and listening for peers.
    ///
    /// Both strategies run concurrently; results are merged into one peer map,
    /// keyed by device id so a peer seen by both is not listed twice.
    pub async fn start(&self) -> Result<()> {
        info!(
            service = SERVICE_TYPE,
            port = self.advertisement.port,
            "starting discovery"
        );

        // TODO(Phase 3, SRS §108): wire the mdns-sd ServiceDaemon here.
        //   let mdns = ServiceDaemon::new()?;
        //   mdns.register(service_info)?;
        //   spawn browse task -> peers.insert(...)
        // Until then, discovery reports no peers rather than inventing any.

        Ok(())
    }

    /// Records a peer, applying the rate limiter and the TTL.
    pub async fn observe_peer(&self, peer: Peer, source: &str) -> bool {
        let now = std::time::Instant::now();

        // Scoped so the guard is released before the `.await` below; holding a
        // std `Mutex` across an await point is how a runtime deadlocks.
        let allowed = {
            let mut limiter = match self.limiter.lock() {
                Ok(guard) => guard,
                // Poisoning means another thread panicked mid-update. The
                // limiter's data is still sound, so recovering is better than
                // taking discovery down with it.
                Err(poisoned) => poisoned.into_inner(),
            };
            limiter.allow(source, now)
        };

        if !allowed {
            warn!(
                source,
                "discovery rate limit exceeded; dropping advertisement"
            );
            return false;
        }
        let mut peers = self.peers.write().await;
        peers.insert(peer.device_id.clone(), (peer, now));
        true
    }

    /// Current peer list, with expired entries dropped.
    pub async fn peers(&self) -> Vec<Peer> {
        let now = std::time::Instant::now();
        let mut peers = self.peers.write().await;
        peers.retain(|_, (_, seen)| now.duration_since(*seen) < PEER_TTL);

        let mut list: Vec<Peer> = peers.values().map(|(p, _)| p.clone()).collect();
        // Best link first, then by name, so the UI order is stable across ticks.
        list.sort_by(|a, b| {
            b.link
                .preference()
                .cmp(&a.link.preference())
                .then_with(|| a.display_name.cmp(&b.display_name))
        });
        list
    }

    pub async fn forget(&self, device_id: &str) {
        self.peers.write().await.remove(device_id);
    }

    /// Probes the link-local subnet when mDNS has produced nothing.
    ///
    /// This is the mDNS-independent path required by FR-DISC-003, over the
    /// link-local addressing FR-DISC-004 mandates. It is deliberately
    /// only run when mDNS is silent — probing a /16 is cheap but not free, and
    /// on a normal LAN mDNS gets there first.
    pub async fn probe_link_local(&self) -> Result<Vec<Peer>> {
        debug!("probing link-local for peers; mDNS produced nothing");
        // TODO(Phase 3): send a discovery datagram to the limited broadcast
        // address on each usable interface, and to the all-hosts multicast
        // group, then collect unicast replies.
        //   - 255.255.255.255:DEFAULT_PORT
        //   - ff02::1%DEFAULT_PORT (per-interface)
        // Bounded concurrency, short timeout, no retry storm.
        Err(Error::PeerNotFound)
    }
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
}
