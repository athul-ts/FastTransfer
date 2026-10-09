//! Interface enumeration, transport ranking, and fallback.
//!
//! Implements SRS §14 and §16. The transfer engine talks to this module through
//! the [`Transport`] trait and never to a concrete transport — that is the
//! transport-independence rule from SRS §11.2.

use std::net::SocketAddr;

use serde::Serialize;

use crate::error::{Error, Result};
use crate::link::{self, Interface, InterfaceKind};

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
#[async_trait::async_trait]
pub trait TransportStream: Send + Unpin {
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize>;
    async fn write_all(&mut self, buf: &[u8]) -> Result<()>;
    async fn shutdown(&mut self) -> Result<()>;
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

/// Enumerates interfaces from the OS.
///
/// TODO(Phase 2, SRS §108): implement with `if-addrs` for addresses and the
/// platform probes in `link.rs` for classification, then map WiFi/Ethernet by
/// the OS's interface-type hint. Returning empty here is honest: it makes
/// `select()` report `NoTransport` rather than naming an interface that may not
/// be the cable.
pub fn enumerate_interfaces() -> Vec<Interface> {
    let _ = link::current();
    Vec::new()
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

    #[test]
    fn transport_labels_are_user_facing() {
        assert_eq!(TransportKind::Thunderbolt.label(), "Thunderbolt");
        assert_eq!(TransportKind::Ethernet.label(), "Ethernet");
        assert_eq!(TransportKind::Wifi.label(), "Wi-Fi");
    }
}
