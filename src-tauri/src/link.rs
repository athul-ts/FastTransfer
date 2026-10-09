//! Cable classification — the heart of the product.
//!
//! Implements SRS §4 (what a cable actually means) and §17 (link layer).
//!
//! The rule this module exists to enforce:
//!
//! > A USB-C connector is not a capability. Report `THUNDERBOLT` only when the
//! > operating system exposes a Thunderbolt-backed network interface that can
//! > carry IP traffic.
//!
//! Detecting the platform-specific interface is stubbed per-OS below; the
//! classification policy on top of it is implemented and tested.

use serde::{Deserialize, Serialize};

/// What a physically attached cable can actually do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LinkKind {
    /// Thunderbolt 3/4/5 networking is available. The target case.
    Thunderbolt,
    /// USB4 exposing a network path. Usable, but not Thunderbolt.
    Usb4Network,
    /// A cable that carries data but exposes no network interface.
    UsbDataOnly,
    /// A cable present with no data path at all.
    ChargeOnly,
    /// Nothing attached.
    None,
}

impl LinkKind {
    /// Whether this link can carry a file transfer.
    pub fn is_usable(self) -> bool {
        matches!(self, LinkKind::Thunderbolt | LinkKind::Usb4Network)
    }

    /// Ranking weight for transport selection (SRS FR-TRANS-004). Higher wins.
    pub fn preference(self) -> u8 {
        match self {
            LinkKind::Thunderbolt => 100,
            LinkKind::Usb4Network => 90,
            LinkKind::UsbDataOnly => 0,
            LinkKind::ChargeOnly => 0,
            LinkKind::None => 0,
        }
    }

    /// Maps a cable classification onto the interface medium it implies.
    pub fn as_interface_kind(self) -> InterfaceKind {
        match self {
            LinkKind::Thunderbolt => InterfaceKind::Thunderbolt,
            LinkKind::Usb4Network => InterfaceKind::Usb4Network,
            LinkKind::UsbDataOnly => InterfaceKind::UsbDataOnly,
            // A charge-only or absent cable tells us nothing about any interface;
            // both collapse to "unclassified".
            LinkKind::ChargeOnly | LinkKind::None => InterfaceKind::Other,
        }
    }
}

/// What the OS told us about an attached cable.
///
/// This is the raw observation. Turning it into a [`LinkKind`] is
/// [`classify`]'s job, and the two are kept separate so the policy is testable
/// without any hardware.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkObservation {
    /// A cable is physically attached.
    pub cable_present: bool,
    /// The OS exposes a Thunderbolt-backed interface carrying IP.
    pub thunderbolt_network: bool,
    /// The OS exposes a USB4-backed interface carrying IP.
    pub usb4_network: bool,
    /// The cable is carrying data of some kind (USB device enumerated).
    pub usb_data: bool,
    /// The controller reports Thunderbolt capability, regardless of networking.
    pub thunderbolt_capable: bool,
}

/// Turns an observation into the classification the UI reports.
///
/// Pure function — no I/O, no platform calls — so it is exhaustively testable.
pub fn classify(obs: &LinkObservation) -> LinkKind {
    if obs.thunderbolt_network {
        LinkKind::Thunderbolt
    } else if obs.usb4_network {
        LinkKind::Usb4Network
    } else if !obs.cable_present {
        LinkKind::None
    } else if obs.usb_data {
        LinkKind::UsbDataOnly
    } else {
        LinkKind::ChargeOnly
    }
}

/// The user-facing explanation for a classification.
///
/// SRS FR-LINK-007 requires that an unusable link explains itself. A
/// `Thunderbolt`-capable controller paired with a data-only cable is the case
/// users find most confusing, so it gets the most specific message.
pub fn explain(kind: LinkKind, obs: &LinkObservation) -> LinkExplanation {
    match kind {
        LinkKind::Thunderbolt => LinkExplanation {
            headline: "Thunderbolt".into(),
            detail: "Connected over Thunderbolt networking.".into(),
            remedy: None,
            usable: true,
        },
        LinkKind::Usb4Network => LinkExplanation {
            headline: "USB4".into(),
            detail: "Connected over USB4 networking.".into(),
            remedy: None,
            usable: true,
        },
        LinkKind::UsbDataOnly => LinkExplanation {
            headline: "USB-C cable detected".into(),
            detail: "This cable carries data but does not provide a network path, \
                     so FastTransfer cannot transfer over it."
                .into(),
            remedy: Some(if obs.thunderbolt_capable {
                "Both computers support Thunderbolt. Use a Thunderbolt cable.".into()
            } else {
                "Use a Thunderbolt cable, or connect both computers to the same network.".into()
            }),
            usable: false,
        },
        LinkKind::ChargeOnly => LinkExplanation {
            headline: "USB-C cable detected".into(),
            detail: "This cable appears to be charge-only.".into(),
            remedy: Some("Use a cable that carries data, or use Ethernet or Wi-Fi.".into()),
            usable: false,
        },
        LinkKind::None => LinkExplanation {
            headline: "No cable".into(),
            detail: "No cable is connected.".into(),
            remedy: Some("Connect a Thunderbolt cable, or use Ethernet or Wi-Fi.".into()),
            usable: false,
        },
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkExplanation {
    pub headline: String,
    pub detail: String,
    pub remedy: Option<String>,
    pub usable: bool,
}

/// What a network interface physically is.
///
/// Deliberately broader than [`LinkKind`], which describes only what a *cable*
/// can do. Ethernet and Wi-Fi are not cables, so they have no `LinkKind` — and
/// deriving the transport class from `LinkKind` alone made both unreachable as
/// transports, silently reducing the product to cable-only. This type is the
/// one that covers every interface the OS can hand us (SRS §16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum InterfaceKind {
    Thunderbolt,
    Usb4Network,
    Ethernet,
    Wifi,
    /// A cable that carries data but exposes no network path.
    UsbDataOnly,
    /// Loopback, tunnels, bridges — or an interface we could not classify.
    Other,
}

/// A network interface as the OS reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Interface {
    pub name: String,
    pub kind: InterfaceKind,
    /// Link speed in bits per second, where the OS reports it. `None` is common
    /// and is not an error — it just means we cannot show a link rating.
    pub link_speed_bps: Option<u64>,
    pub up: bool,
    pub is_virtual: bool,
    pub addresses: Vec<String>,
}

/// sysfs probe, used on Linux.
///
/// Compiled on every platform so it can be unit-tested against a synthetic
/// sysfs tree; only *called* on Linux.
#[cfg(any(target_os = "linux", test))]
mod sysfs {
    use super::LinkObservation;
    use std::fs;
    use std::path::Path;

    /// Reads a sysfs attribute. `None` for absent or unreadable: a missing
    /// attribute is normal, not an error, and must never panic.
    fn attr(path: &Path) -> Option<String> {
        fs::read_to_string(path).ok().map(|s| s.trim().to_string())
    }

    /// Whether a Thunderbolt or USB4 host controller is exposed, and which
    /// generation it reports (3 = Thunderbolt 3, 4 = USB4).
    ///
    /// `/sys/bus/thunderbolt` exists only when the `thunderbolt` module is bound
    /// to an NHI. Its absence means no controller is exposed to us — a finding
    /// to report, not an error to swallow (FR-LINK-003).
    fn host(root: &Path) -> (bool, Option<u32>) {
        let Ok(entries) = fs::read_dir(root.join("bus/thunderbolt/devices")) else {
            return (false, None);
        };
        for entry in entries.flatten() {
            // The host router is `<domain>-0`; peers are `<domain>-<n>`.
            if !entry.file_name().to_string_lossy().ends_with("-0") {
                continue;
            }
            let generation = attr(&entry.path().join("generation")).and_then(|g| g.parse().ok());
            return (true, generation);
        }
        (false, None)
    }

    /// The Thunderbolt network interface, if the kernel created one.
    ///
    /// `thunderbolt-net` binds to the host router and creates exactly one
    /// interface, conventionally `thunderbolt0` (SRS §17). The driver is the
    /// real signal; the name is checked too because an interface renamed by
    /// udev keeps its driver.
    fn netdev(root: &Path) -> Option<String> {
        let entries = fs::read_dir(root.join("class/net")).ok()?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                continue;
            }
            let driver = fs::read_link(entry.path().join("device/driver"))
                .ok()
                .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));
            if driver.as_deref() == Some("thunderbolt-net") || name.starts_with("thunderbolt") {
                return Some(name);
            }
        }
        None
    }

    /// Whether a USB-C partner is attached, per the Type-C class.
    fn typec_partner(root: &Path) -> bool {
        let Ok(entries) = fs::read_dir(root.join("class/typec")) else {
            return false;
        };
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with("-partner"))
    }

    /// Probes a sysfs root. Takes the root as a parameter so the whole probe can
    /// be exercised against a temporary directory, with no hardware present.
    pub(super) fn observe_at(root: &Path) -> LinkObservation {
        let (capable, generation) = host(root);
        let network = netdev(root).is_some();

        // Thunderbolt and USB4 share one kernel driver; the host router's
        // `generation` is what separates them. An unreadable generation still
        // means a Thunderbolt-bus network path exists, so it is reported as
        // Thunderbolt — the two are equally usable and the claim is true either
        // way, which is what FR-LINK-003 actually protects.
        let usb4 = generation == Some(4);

        LinkObservation {
            cable_present: network || typec_partner(root),
            thunderbolt_network: network && !usb4,
            usb4_network: network && usb4,
            // A Type-C partner with no network path may be carrying USB data or
            // only power, and sysfs does not distinguish them for us. Claiming
            // USB data would be a guess, so we do not claim it: the link
            // classifies as ChargeOnly, whose message is hedged ("appears to
            // be") rather than asserting something we have not observed.
            usb_data: false,
            thunderbolt_capable: capable,
        }
    }
}

/// Device-tree probe, used on Windows.
///
/// Windows identifies a network adapter by a display name, and a display name
/// can be changed. Matching on it is therefore the one thing an honest link
/// report must not rest on (SRS FR-LINK-003). This reads the *device tree*
/// instead: the class, the bound driver service, and the hardware IDs a device
/// reports about itself.
///
/// Two rules here were arrived at by measurement rather than assumption, and
/// both matter:
///
/// - `PCI\CC_0880` is **not** a Thunderbolt marker. It is the generic PCI class
///   08/80 compatible ID, and unrelated System devices carry it — this
///   machine's Intel GNA Scoring Accelerator, for one. Intel's Thunderbolt INF
///   does list `...&CC_0880` entries, but always bound to a specific device ID,
///   so the class code alone cannot identify a controller.
/// - Thunderbolt 3 has no distinguishing PCI class code at all. On Windows 11
///   the host side is USB4 hardware bound to Microsoft's connection-manager
///   services; on Windows 10 a discrete controller appears by name. So
///   capability and classification draw on different signals on purpose.
///
/// Compiled on every platform, and split from the OS call that feeds it, so the
/// classification rules are unit-tested against synthetic device trees rather
/// than against whatever hardware the test machine happens to have.
#[cfg(any(target_os = "windows", test))]
mod pnp {
    use super::{iface, LinkObservation};

    /// A Thunderbolt-family controller, as the device tree describes it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum Controller {
        /// A discrete Thunderbolt 1–3 controller.
        Thunderbolt,
        /// USB4 host-side hardware.
        Usb4,
    }

    /// PCI class code for the USB4 host interface. Unlike `0x0880` this one is
    /// specific: it is the Serial Bus / USB / USB4 host-interface class.
    const CC_USB4: &str = "CC_0C0340";

    /// Driver services that bind USB4 host-side hardware.
    ///
    /// `Usb4HostRouter` is the host router; `Usb4P2PNetAdapter` is — per
    /// Microsoft's USB4 connection-manager documentation — the virtual network
    /// adapter for host-to-host connections, which is exactly the cable path
    /// this product exists to use.
    const USB4_HOST_SERVICES: [&str; 2] = ["Usb4HostRouter", "Usb4P2PNetAdapter"];

    /// One present device, reduced to the fields that identify it.
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub(super) struct DeviceFacts {
        /// Device class name (`SPDRP_CLASS`): `Net`, `USB4`, `System`, …
        pub class: Option<String>,
        /// Display name (`SPDRP_FRIENDLYNAME`).
        pub name: Option<String>,
        /// Bound driver service (`SPDRP_SERVICE`).
        pub service: Option<String>,
        /// Hardware IDs and compatible IDs (`SPDRP_HARDWAREID`,
        /// `SPDRP_COMPATIBLEIDS`).
        pub ids: Vec<String>,
    }

    impl DeviceFacts {
        /// The controller this device is, if any.
        ///
        /// Narrow on purpose. This drives the link classification the UI
        /// reports, so it accepts only device-bound evidence — device class,
        /// bound driver service, hardware IDs. A display name is never enough
        /// here: an adapter renamed to look like Thunderbolt must not become
        /// Thunderbolt (FR-LINK-003).
        ///
        /// USB4 is tested before Thunderbolt because USB4 hardware also reports
        /// Thunderbolt-compatible IDs, and reporting Thunderbolt for a USB4
        /// link would overstate it.
        pub(super) fn controller(&self) -> Option<Controller> {
            let class_is = |n: &str| {
                self.class
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(n))
            };
            let service_is = |n: &str| {
                self.service
                    .as_deref()
                    .is_some_and(|s| s.eq_ignore_ascii_case(n))
            };
            let id_has = |n: &str| self.ids.iter().any(|id| upper(id).contains(n));

            if class_is("USB4")
                || USB4_HOST_SERVICES.iter().any(|s| service_is(s))
                || id_has(CC_USB4)
                || self.ids.iter().any(|id| upper(id).starts_with("USB4\\"))
            {
                return Some(Controller::Usb4);
            }
            if class_is("Thunderbolt") {
                return Some(Controller::Thunderbolt);
            }
            None
        }

        /// Whether this device is evidence that the *machine* has
        /// Thunderbolt-family hardware.
        ///
        /// Broader than [`Self::controller`], because it only drives the remedy
        /// text a user reads: a false positive costs a slightly over-specific
        /// suggestion, not a false claim about a live link. It adds the
        /// INF-supplied device name, which for a Windows 10 discrete
        /// Thunderbolt controller is the only signal available, and which —
        /// unlike a network adapter's alias — is not user-editable without
        /// administrator registry work.
        pub(super) fn hints_capability(&self) -> bool {
            self.controller().is_some()
                || self
                    .name
                    .as_deref()
                    .is_some_and(|n| n.to_ascii_lowercase().contains("thunderbolt"))
        }
    }

    /// Hardware IDs are matched case-insensitively.
    fn upper(id: &str) -> String {
        id.to_ascii_uppercase()
    }

    /// Builds an observation from a device tree and the live interface names.
    ///
    /// The live interface *alias* is only the join key between the two views;
    /// the device's own identity is the evidence. An adapter someone has
    /// renamed is still identified, which is the whole improvement over
    /// matching on the alias alone.
    pub(super) fn observe_from_devices(
        devices: &[DeviceFacts],
        interfaces: &[String],
    ) -> LinkObservation {
        // Capability is a property of the machine, not of the cable: hardware in
        // the tree means Thunderbolt is available to plug into, whether or not
        // anything is plugged in right now.
        let capable = devices.iter().any(|d| d.hints_capability());

        let describes = |interface: &str| {
            devices.iter().find(|d| {
                d.name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(interface))
            })
        };

        let backing = interfaces
            .iter()
            .find_map(|interface| describes(interface).and_then(DeviceFacts::controller));

        let (thunderbolt_network, usb4_network) = match backing {
            Some(Controller::Usb4) => (false, true),
            Some(Controller::Thunderbolt) => (true, false),
            // No interface is backed by a Thunderbolt-family device. The alias is
            // consulted as a last resort, and only for interfaces the device tree
            // does not describe at all: where it describes one and that device's
            // identity says otherwise, the device's own answer wins.
            None => (
                interfaces
                    .iter()
                    .any(|i| iface::is_thunderbolt_name(i) && describes(i).is_none()),
                false,
            ),
        };

        LinkObservation {
            cable_present: thunderbolt_network || usb4_network,
            thunderbolt_network,
            usb4_network,
            // Whether a Type-C partner is carrying data while exposing no
            // network path is not visible in the device tree either, and
            // guessing it would assert something we have not observed.
            usb_data: false,
            thunderbolt_capable: capable,
        }
    }
}

/// The OS call behind [`pnp`] on Windows.
///
/// Isolated here so that exactly one function in the crate does FFI and
/// everything above it is ordinary, testable Rust.
#[cfg(target_os = "windows")]
mod setupapi {
    use super::pnp::DeviceFacts;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation as di;
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

    /// Device classes worth reading identity strings from.
    ///
    /// A device outside this set is skipped after its class read, so an
    /// unrelated device costs one registry call instead of four.
    const RELEVANT: [&str; 4] = ["net", "usb4", "thunderbolt", "system"];

    /// Reads one property, returning the raw UTF-16 it wrote.
    ///
    /// An absent property yields an empty buffer: devices routinely lack some
    /// of these, and that is normal rather than an error.
    fn property(
        set: di::HDEVINFO,
        device: &di::SP_DEVINFO_DATA,
        which: di::SETUP_DI_REGISTRY_PROPERTY,
    ) -> Vec<u16> {
        let mut buffer = vec![0u16; 1024];
        let mut needed = 0u32;
        // SAFETY: `set` and `device` are live handles owned by the caller for
        // the duration of the call; the buffer is valid for `buffer.len()` u16s
        // and that byte length is what the API is told.
        let ok = unsafe {
            di::SetupDiGetDeviceRegistryPropertyW(
                set,
                device,
                which,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast::<u8>(),
                (buffer.len() * std::mem::size_of::<u16>()) as u32,
                &mut needed,
            )
        };
        if ok == 0 {
            return Vec::new();
        }
        buffer.truncate((needed as usize / std::mem::size_of::<u16>()).min(buffer.len()));
        buffer
    }

    /// The single string in a `REG_SZ` buffer.
    fn one(buffer: Vec<u16>) -> Option<String> {
        let text = String::from_utf16_lossy(&buffer);
        let text = text.trim_end_matches('\0').trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    /// Every string in a `REG_MULTI_SZ` buffer.
    fn many(buffer: Vec<u16>) -> Vec<String> {
        String::from_utf16_lossy(&buffer)
            .split('\0')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// Every present device in this machine's device tree.
    ///
    /// Fails closed: an unreadable tree returns an empty list, which reads as
    /// "no capability observed" rather than as a capability we cannot back up.
    pub(super) fn present_devices() -> Vec<DeviceFacts> {
        let mut devices = Vec::new();

        // SAFETY: the handle is checked against the documented invalid value
        // before any use, and released exactly once before returning.
        unsafe {
            let set = di::SetupDiGetClassDevsW(
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                di::DIGCF_ALLCLASSES | di::DIGCF_PRESENT,
            );
            if set == INVALID_HANDLE_VALUE as di::HDEVINFO {
                tracing::debug!("device tree enumeration returned no device set");
                return devices;
            }

            let mut index = 0u32;
            loop {
                // SAFETY: `SP_DEVINFO_DATA` is a plain C struct whose only
                // precondition is a correct `cbSize`, set immediately below.
                let mut info: di::SP_DEVINFO_DATA = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<di::SP_DEVINFO_DATA>() as u32;

                if di::SetupDiEnumDeviceInfo(set, index, &mut info) == 0 {
                    break;
                }
                index += 1;

                let class = one(property(set, &info, di::SPDRP_CLASS));
                let relevant = class
                    .as_deref()
                    .is_some_and(|c| RELEVANT.iter().any(|r| c.eq_ignore_ascii_case(r)));
                if !relevant {
                    continue;
                }

                let mut ids = many(property(set, &info, di::SPDRP_HARDWAREID));
                ids.extend(many(property(set, &info, di::SPDRP_COMPATIBLEIDS)));

                devices.push(DeviceFacts {
                    class,
                    name: one(property(set, &info, di::SPDRP_FRIENDLYNAME)),
                    service: one(property(set, &info, di::SPDRP_SERVICE)),
                    ids,
                });
            }

            di::SetupDiDestroyDeviceInfoList(set);
        }

        devices
    }
}

/// Probe for platforms whose interfaces carry their identity in their name.
///
/// Compiled on every platform so the matching rule can be unit-tested; only
/// *called* on macOS, where the Thunderbolt Bridge is named `bridge0` and no
/// device tree is consulted.
#[cfg(any(target_os = "windows", target_os = "macos", test))]
mod iface {
    #[cfg(any(target_os = "macos", test))]
    use super::LinkObservation;

    /// Names the OS gives a Thunderbolt network path.
    ///
    /// macOS names its Thunderbolt Bridge `bridge0`, and Windows names its
    /// adapter `Thunderbolt(TM) Networking`.
    ///
    /// On Windows this is now the *fallback*, not the evidence: [`super::pnp`]
    /// reads the adapter's hardware identity, and the name is consulted only
    /// when the device tree cannot name the adapter. It still errs toward not
    /// claiming — an unrecognised name yields no Thunderbolt claim, which is
    /// the safe direction for FR-LINK-003.
    pub(super) fn is_thunderbolt_name(name: &str) -> bool {
        let lower = name.to_ascii_lowercase();
        lower.contains("thunderbolt") || lower == "bridge0"
    }

    #[cfg(any(target_os = "macos", test))]
    pub(super) fn observe_from_names<'a>(names: impl Iterator<Item = &'a str>) -> LinkObservation {
        let network = names.into_iter().any(is_thunderbolt_name);
        LinkObservation {
            cable_present: network,
            thunderbolt_network: network,
            usb4_network: false,
            usb_data: false,
            // Interface names cannot say whether a controller exists but is not
            // networking. Reporting `false` is the honest answer: it makes
            // `explain` fall back to general advice rather than assert a
            // capability we have not verified.
            thunderbolt_capable: false,
        }
    }
}

/// Names of this machine's network interfaces, or empty if enumeration fails.
///
/// A failure is reported as "no interfaces", never as an error: the caller then
/// classifies "no cable", which is the honest reading. Guessing here is what
/// FR-LINK-003 forbids.
#[cfg(any(target_os = "windows", target_os = "macos"))]
fn interface_names() -> Vec<String> {
    match if_addrs::get_if_addrs() {
        Ok(list) => list.into_iter().map(|i| i.name).collect(),
        Err(e) => {
            tracing::debug!(error = %e, "interface enumeration failed");
            Vec::new()
        }
    }
}

/// Reads the current cable state from the platform.
///
/// # Platform support
///
/// | OS | Mechanism | Confidence |
/// |---|---|---|
/// | Linux | sysfs: `/sys/bus/thunderbolt/devices`, `/sys/class/net`, `/sys/class/typec` | High |
/// | Windows | SetupAPI device tree (`SPDRP_HARDWAREID`, `SPDRP_COMPATIBLEIDS`) joined to the live interface names | Medium–High |
/// | macOS | Thunderbolt Bridge interface name (`bridge0`) via `if-addrs` | Medium |
///
/// # What this deliberately does not do
///
/// It never guesses in the positive direction. Every path returns "no
/// Thunderbolt" unless the OS positively shows a Thunderbolt-backed interface,
/// because a wrong `THUNDERBOLT` claim is the one failure the product must not
/// have (SRS FR-LINK-003, §4.2).
///
/// Two consequences are worth knowing:
///
/// - A cable that is physically plugged in but provides no data path is
///   invisible to the OS, so it classifies as [`LinkKind::None`] ("no cable")
///   rather than something more specific. The OS genuinely has no state to
///   report.
/// - On Windows the join between the device tree and the interface list is by
///   display name, because that is what `if-addrs` exposes. The *evidence* is
///   the hardware ID, so a renamed adapter is still identified; but an adapter
///   the device tree cannot name falls back to the name heuristic. macOS still
///   reports `thunderbolt_capable` as `false`, needing an IOKit probe that is
///   not yet implemented; the effect there is only that `explain` offers
///   general advice instead of the specific "use a Thunderbolt cable" remedy.
pub fn observe() -> LinkObservation {
    #[cfg(target_os = "linux")]
    let obs = sysfs::observe_at(std::path::Path::new("/sys"));

    #[cfg(target_os = "windows")]
    let obs = pnp::observe_from_devices(&setupapi::present_devices(), &interface_names());

    #[cfg(target_os = "macos")]
    let obs = iface::observe_from_names(interface_names().iter().map(String::as_str));

    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    let obs = LinkObservation::default();

    obs
}

/// Classifies the current link and explains it. This is what the UI calls.
pub fn current() -> (LinkKind, LinkExplanation) {
    let obs = observe();
    let kind = classify(&obs);
    (kind, explain(kind, &obs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thunderbolt_network_classifies_as_thunderbolt() {
        let obs = LinkObservation {
            cable_present: true,
            thunderbolt_network: true,
            thunderbolt_capable: true,
            ..Default::default()
        };
        assert_eq!(classify(&obs), LinkKind::Thunderbolt);
    }

    #[test]
    fn usb_c_connector_alone_is_not_thunderbolt() {
        // The single most important test in this file. A USB-C cable carrying
        // USB data must never be reported as Thunderbolt (SRS §4.2, FR-LINK-003).
        let obs = LinkObservation {
            cable_present: true,
            usb_data: true,
            thunderbolt_capable: true,  // the *controller* supports it...
            thunderbolt_network: false, // ...but no network path is exposed
            ..Default::default()
        };
        assert_eq!(classify(&obs), LinkKind::UsbDataOnly);
        assert_ne!(classify(&obs), LinkKind::Thunderbolt);
    }

    #[test]
    fn charge_only_cable_is_identified() {
        let obs = LinkObservation {
            cable_present: true,
            ..Default::default()
        };
        assert_eq!(classify(&obs), LinkKind::ChargeOnly);
    }

    #[test]
    fn nothing_attached_is_none() {
        assert_eq!(classify(&LinkObservation::default()), LinkKind::None);
    }

    #[test]
    fn usb4_ranks_below_thunderbolt_but_is_still_usable() {
        assert!(LinkKind::Usb4Network.is_usable());
        assert!(LinkKind::Usb4Network.preference() < LinkKind::Thunderbolt.preference());
    }

    #[test]
    fn unusable_links_always_offer_a_remedy() {
        // SRS FR-LINK-007: an unusable link must explain why and what to do.
        for kind in [LinkKind::UsbDataOnly, LinkKind::ChargeOnly, LinkKind::None] {
            let explanation = explain(kind, &LinkObservation::default());
            assert!(!explanation.usable);
            assert!(
                explanation.remedy.is_some(),
                "{kind:?} gave no remedy to the user"
            );
            assert!(!explanation.detail.is_empty());
        }
    }

    #[test]
    fn data_only_cable_mentions_thunderbolt_when_both_sides_support_it() {
        let obs = LinkObservation {
            cable_present: true,
            usb_data: true,
            thunderbolt_capable: true,
            ..Default::default()
        };
        let explanation = explain(LinkKind::UsbDataOnly, &obs);
        let remedy = explanation.remedy.unwrap();
        assert!(
            remedy.to_lowercase().contains("thunderbolt"),
            "expected a Thunderbolt remedy, got: {remedy}"
        );
    }

    #[test]
    fn usable_links_report_no_remedy() {
        let explanation = explain(LinkKind::Thunderbolt, &LinkObservation::default());
        assert!(explanation.usable);
        assert!(explanation.remedy.is_none());
    }

    // --- Platform probe -----------------------------------------------------
    //
    // `observe_at` takes its sysfs root as a parameter, so these build a
    // synthetic tree and run on any host. No Linux required, no cable required.

    fn fake_sysfs() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    /// Creates the `<domain>-0` host router entry with a given generation.
    fn host_router(root: &std::path::Path, generation: u32) {
        let dir = root.join("bus/thunderbolt/devices/0-0");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("generation"), generation.to_string()).expect("write");
    }

    #[test]
    fn absent_sysfs_reports_no_cable() {
        // The development machine behaves exactly like this: a port shaped like
        // Thunderbolt with nothing behind it. The probe must report the absence
        // rather than invent a link.
        let dir = fake_sysfs();
        let obs = sysfs::observe_at(dir.path());
        assert_eq!(obs, LinkObservation::default());
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[test]
    fn thunderbolt_netdev_classifies_as_thunderbolt() {
        let dir = fake_sysfs();
        host_router(dir.path(), 3);
        std::fs::create_dir_all(dir.path().join("class/net/thunderbolt0")).expect("mkdir");

        let obs = sysfs::observe_at(dir.path());
        assert!(obs.thunderbolt_capable);
        assert_eq!(classify(&obs), LinkKind::Thunderbolt);
    }

    #[test]
    fn usb4_host_classifies_as_usb4_not_thunderbolt() {
        // Generation 4 is USB4. Promising Thunderbolt here would be a claim the
        // hardware does not support (FR-LINK-003).
        let dir = fake_sysfs();
        host_router(dir.path(), 4);
        std::fs::create_dir_all(dir.path().join("class/net/thunderbolt0")).expect("mkdir");

        let obs = sysfs::observe_at(dir.path());
        assert_eq!(classify(&obs), LinkKind::Usb4Network);
        assert!(classify(&obs).is_usable());
    }

    #[test]
    fn controller_without_a_cable_is_capable_but_unlinked() {
        // A Thunderbolt controller with nothing plugged into it. Not a live
        // link — but the capability is real and drives the remedy text.
        let dir = fake_sysfs();
        host_router(dir.path(), 3);

        let obs = sysfs::observe_at(dir.path());
        assert!(obs.thunderbolt_capable);
        assert!(!obs.cable_present);
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[test]
    fn typec_partner_without_a_network_path_is_unusable() {
        let dir = fake_sysfs();
        std::fs::create_dir_all(dir.path().join("class/typec/port0-partner")).expect("mkdir");

        let obs = sysfs::observe_at(dir.path());
        assert!(obs.cable_present);
        assert_eq!(classify(&obs), LinkKind::ChargeOnly);
        assert!(!classify(&obs).is_usable());
    }

    #[test]
    fn ordinary_interfaces_are_never_thunderbolt() {
        // The regression that would make the product lie about every laptop.
        let dir = fake_sysfs();
        std::fs::create_dir_all(dir.path().join("class/net/wlan0")).expect("mkdir");
        std::fs::create_dir_all(dir.path().join("class/net/enp0s31f6")).expect("mkdir");

        let obs = sysfs::observe_at(dir.path());
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[cfg(unix)]
    #[test]
    fn a_renamed_thunderbolt_netdev_is_still_found_by_driver() {
        // udev renaming must not hide the link: the driver is the real signal
        // and the name is only a convenience.
        let dir = fake_sysfs();
        host_router(dir.path(), 3);

        let device = dir.path().join("class/net/enp9s0/device");
        std::fs::create_dir_all(&device).expect("mkdir");
        let driver_dir = dir.path().join("bus/pci/drivers/thunderbolt-net");
        std::fs::create_dir_all(&driver_dir).expect("mkdir");
        std::os::unix::fs::symlink(&driver_dir, device.join("driver")).expect("symlink");

        let obs = sysfs::observe_at(dir.path());
        assert_eq!(classify(&obs), LinkKind::Thunderbolt);
    }

    #[test]
    fn windows_style_names_are_matched_and_ordinary_ones_are_not() {
        assert!(iface::is_thunderbolt_name("Thunderbolt(TM) Networking"));
        assert!(iface::is_thunderbolt_name("bridge0"));
        for ordinary in [
            "Wi-Fi",
            "Ethernet",
            "McAfee VPN",
            "vEthernet (Default Switch)",
            "lo",
        ] {
            assert!(
                !iface::is_thunderbolt_name(ordinary),
                "{ordinary:?} was mistaken for a Thunderbolt interface"
            );
        }
    }

    #[test]
    fn a_machine_with_no_thunderbolt_adapter_reports_no_cable() {
        // Exactly what the Windows development machine reports: Wi-Fi, a VPN
        // adapter, and a Hyper-V switch — no Thunderbolt anywhere.
        let obs = iface::observe_from_names(
            ["Wi-Fi", "McAfee VPN", "vEthernet (Default Switch)"].into_iter(),
        );
        assert_eq!(classify(&obs), LinkKind::None);
    }

    // --- Windows device tree ------------------------------------------------
    //
    // `observe_from_devices` takes the device tree as a parameter, so these
    // build synthetic trees and run on any host. No Windows, no cable.

    fn device(class: &str, name: &str, ids: &[&str]) -> pnp::DeviceFacts {
        pnp::DeviceFacts {
            class: Some(class.to_string()),
            name: Some(name.to_string()),
            service: None,
            ids: ids.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    /// A device bound to a driver service, which is device-bound evidence.
    fn served(class: &str, name: &str, service: &str, ids: &[&str]) -> pnp::DeviceFacts {
        pnp::DeviceFacts {
            service: Some(service.to_string()),
            ..device(class, name, ids)
        }
    }

    fn interface_names_of(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn the_reference_windows_machine_reports_no_thunderbolt() {
        // The measured hardware (SRS §116.1.1): Intel xHCI controllers, Wi-Fi,
        // a VPN adapter — and no Thunderbolt or USB4 controller anywhere. The
        // device-level probe must reproduce that finding rather than infer a
        // controller from the port's shape.
        //
        // The GNA module is carried here deliberately: it is a real device on
        // this machine reporting `PCI\CC_0880`, the class code this probe
        // originally — and wrongly — treated as a Thunderbolt marker.
        let tree = [
            device(
                "System",
                "Intel(R) GNA Scoring Accelerator module",
                &[
                    "PCI\\VEN_8086&DEV_774C&CC_088000",
                    "PCI\\VEN_8086&DEV_774C&CC_0880",
                ],
            ),
            device(
                "USB",
                "Intel(R) USB 3.1 eXtensible Host Controller",
                &["PCI\\VEN_8086&DEV_7E7D&CC_0C0330"],
            ),
            device(
                "Net",
                "Intel(R) Wi-Fi 6E AX211 160MHz",
                &["PCI\\VEN_8086&DEV_51F0&CC_028000"],
            ),
            device("Net", "McAfee VPN", &["ROOT\\NET\\0000"]),
        ];
        let obs = pnp::observe_from_devices(
            &tree,
            &interface_names_of(&["Wi-Fi", "McAfee VPN", "vEthernet (Default Switch)"]),
        );
        assert!(
            !obs.thunderbolt_capable,
            "claimed Thunderbolt capability with no controller present: {obs:?}"
        );
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[test]
    fn a_usb4_p2p_adapter_classifies_as_usb4_not_thunderbolt() {
        // `Usb4P2PNetAdapter` is Microsoft's host-to-host network adapter — the
        // cable path this product exists to use. USB4 is usable but is not
        // Thunderbolt, and saying otherwise would overstate the link.
        let tree = [served(
            "Net",
            "USB4(TM) P2P Network Adapter",
            "Usb4P2PNetAdapter",
            &["USB4\\NetworkingChild"],
        )];
        let obs = pnp::observe_from_devices(
            &tree,
            &interface_names_of(&["USB4(TM) P2P Network Adapter"]),
        );
        assert!(obs.thunderbolt_capable);
        assert_eq!(classify(&obs), LinkKind::Usb4Network);
        assert!(classify(&obs).is_usable());
    }

    #[test]
    fn a_renamed_adapter_is_still_identified_by_its_bound_driver() {
        // The regression the device-level probe exists to prevent: renaming the
        // adapter must not be able to hide the link, because the alias is only
        // the join key and the bound driver service is the evidence.
        let tree = [served("Net", "Link 2", "Usb4P2PNetAdapter", &[])];
        let obs = pnp::observe_from_devices(&tree, &interface_names_of(&["Link 2"]));
        assert_eq!(classify(&obs), LinkKind::Usb4Network);
    }

    #[test]
    fn a_name_is_never_believed_over_the_device_tree() {
        // An adapter *called* Thunderbolt whose device says it is a Realtek
        // Ethernet controller. The device wins: a name must not be able to
        // manufacture a Thunderbolt link (FR-LINK-003).
        let tree = [device(
            "Net",
            "Thunderbolt(TM) Networking",
            &["PCI\\VEN_10EC&DEV_8168&CC_020000"],
        )];
        let obs =
            pnp::observe_from_devices(&tree, &interface_names_of(&["Thunderbolt(TM) Networking"]));
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[test]
    fn an_ordinary_ethernet_adapter_is_never_thunderbolt() {
        let tree = [device(
            "Net",
            "Realtek PCIe GbE Family Controller",
            &["PCI\\VEN_10EC&DEV_8168&CC_020000"],
        )];
        let obs = pnp::observe_from_devices(&tree, &interface_names_of(&["Ethernet"]));
        assert!(!obs.thunderbolt_capable);
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[test]
    fn a_host_router_with_no_adapter_is_capable_but_unlinked() {
        // USB4 host-side hardware present with nothing plugged in. Not a live
        // link — but the capability is real, and it now reaches the user through
        // the remedy text, which it could not while the field was hardcoded
        // `false` on Windows.
        let tree = [served(
            "USB4",
            "USB4(TM) Host Router",
            "Usb4HostRouter",
            &["PCI\\VEN_8086&DEV_7EC0&CC_0C0340"],
        )];
        let obs = pnp::observe_from_devices(&tree, &interface_names_of(&["Wi-Fi", "Ethernet"]));

        assert!(obs.thunderbolt_capable);
        assert!(!obs.cable_present);
        assert_eq!(classify(&obs), LinkKind::None);

        let explanation = explain(
            LinkKind::UsbDataOnly,
            &LinkObservation {
                cable_present: true,
                usb_data: true,
                thunderbolt_capable: obs.thunderbolt_capable,
                ..Default::default()
            },
        );
        let remedy = explanation
            .remedy
            .expect("unusable link must offer a remedy");
        assert!(
            remedy.to_lowercase().contains("thunderbolt"),
            "expected the specific Thunderbolt remedy, got: {remedy}"
        );
    }

    #[test]
    fn a_discrete_controller_known_only_by_name_still_counts_as_capable() {
        // Windows 10 exposes a discrete Thunderbolt controller by name and
        // little else, and Thunderbolt 3 has no distinguishing PCI class code to
        // fall back on. That is enough to offer the right remedy — but not
        // enough to classify a link, which is why it feeds capability only.
        let tree = [device(
            "System",
            "Thunderbolt(TM) Controller",
            &["PCI\\VEN_8086&DEV_15EF"],
        )];
        let obs = pnp::observe_from_devices(&tree, &interface_names_of(&["Wi-Fi"]));
        assert!(obs.thunderbolt_capable);
        assert_eq!(classify(&obs), LinkKind::None);
    }

    #[test]
    fn an_interface_absent_from_the_device_tree_still_falls_back_to_its_name() {
        // Not every adapter is described by the tree we can read. Where it is
        // silent, the alias remains the best evidence available.
        let tree = [device(
            "Net",
            "Realtek PCIe GbE Family Controller",
            &["PCI\\VEN_10EC&DEV_8168&CC_020000"],
        )];
        let obs = pnp::observe_from_devices(
            &tree,
            &interface_names_of(&["Ethernet", "Thunderbolt(TM) Networking"]),
        );
        assert_eq!(classify(&obs), LinkKind::Thunderbolt);
    }

    #[test]
    fn the_live_probe_is_self_consistent() {
        // Runs against the real machine, so the values differ per host — hence
        // an invariant rather than a fixed classification. A Thunderbolt claim
        // with no cable, or two network kinds at once, would be a bug wherever
        // it ran.
        let obs = observe();

        // Captured by the test harness normally; shown by `cargo test -- --
        // nocapture`. Printed so a classification dispute on a real machine can
        // be settled from the output instead of by repeating the cable check.
        println!("live probe: {obs:?} -> {:?}", classify(&obs));

        assert!(
            !(obs.thunderbolt_network && obs.usb4_network),
            "both Thunderbolt and USB4 claimed at once: {obs:?}"
        );
        if obs.thunderbolt_network || obs.usb4_network {
            assert!(
                obs.cable_present,
                "network path claimed with no cable present"
            );
        }
    }
}
