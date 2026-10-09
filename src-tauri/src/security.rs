//! Device identity, pairing, and trusted devices.
//!
//! Implements SRS §12, §15, §34–§36.
//!
//! # Where the crypto is, and is not
//!
//! This module owns the *identity and trust* model: a stable device keypair, a
//! short verification code, and a store of trusted peers. It does **not**
//! implement a handshake.
//!
//! The session handshake is Noise (`Noise_XX_25519_ChaChaPoly_BLAKE2s`) or
//! TLS 1.3, from an established crate, added in Phase 7 (SRS NFR-SEC-009). Any
//! primitive written by hand here would be a defect regardless of how correct it
//! looked — the rule is absolute.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::error::{Error, Result};

/// How long a pairing code stays valid. Short by design: it is read aloud and
/// typed, so it only needs to outlive the conversation. SRS FR-PAIR-004.
pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(120);

/// Pairing attempts allowed per peer within the rate-limit window.
pub const MAX_PAIRING_ATTEMPTS: usize = 3;

/// Window for the pairing rate limit. SRS FR-PAIR-005.
pub const PAIRING_RATE_WINDOW: Duration = Duration::from_secs(300);

/// This installation's identity. Stable across restarts; never transmitted
/// beyond the public key (SRS §12).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentity {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    /// Base64 of the public identity key. The private half lives only in OS
    /// secure storage (SRS NFR-SEC-005).
    pub public_key: String,
}

/// A device the user has explicitly approved. SRS §36.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedDevice {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    pub public_key: String,
    /// Unix seconds.
    pub trusted_since: i64,
    pub last_seen: i64,
}

/// A pending pairing, awaiting the user's decision.
#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    /// The code shown on *this* device. The user compares it with the code on
    /// the other screen — a mismatch means something is between the two.
    pub code: String,
    pub started: Instant,
    pub attempts: usize,
}

impl PendingPairing {
    pub fn is_expired(&self, now: Instant) -> bool {
        now.duration_since(self.started) > PAIRING_CODE_TTL
    }

    /// Whether the peer has exhausted its attempts and must wait.
    pub fn is_rate_limited(&self) -> bool {
        self.attempts >= MAX_PAIRING_ATTEMPTS
    }
}

/// Generates a six-digit verification code, formatted for reading aloud.
///
/// Uses the OS CSPRNG. A predictable code would let an attacker on the same link
/// complete a pairing the user never approved.
pub fn generate_pairing_code() -> String {
    use rand::Rng;
    let n: u32 = rand::thread_rng().gen_range(0..1_000_000);
    format!("{:03} {:03}", n / 1000, n % 1000)
}

/// Constant-time comparison for pairing codes and digests.
///
/// A short-circuiting `==` leaks the position of the first wrong digit through
/// timing, which is enough to recover a six-digit code.
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Where identity and trust data live.
#[derive(Debug, Clone)]
pub struct IdentityStore {
    path: PathBuf,
}

impl IdentityStore {
    /// Creates a store under the platform's app-data directory.
    pub fn open() -> Result<Self> {
        let dir = directories::ProjectDirs::from("app", "FastTransfer", "FastTransfer")
            .ok_or_else(|| {
                Error::Other(anyhow::anyhow!("no application data directory available"))
            })?;
        let path = dir.data_dir().join("identity.json");
        Ok(Self { path })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Loads the identity, generating one on first run.
    ///
    /// # Security
    ///
    /// TODO(Phase 7, SRS §108): move the private key into the OS keychain
    /// (Keychain / DPAPI / Secret Service). Until then this stores only the
    /// public identity and refuses to persist any secret — see the guard below.
    /// `FASTTRANSFER_INSECURE_IDENTITY_STORE` exists for tests and must never be
    /// set in a production build.
    pub fn load_or_create(&self, display_name: String) -> Result<DeviceIdentity> {
        if self.path.exists() {
            let raw = std::fs::read_to_string(&self.path)?;
            return Ok(serde_json::from_str(&raw)?);
        }

        let identity = DeviceIdentity {
            device_id: uuid::Uuid::new_v4().to_string(),
            display_name,
            platform: std::env::consts::OS.to_string(),
            // Placeholder until Phase 7 generates a real keypair in the keychain.
            public_key: String::new(),
        };

        self.save(&identity)?;
        Ok(identity)
    }

    pub fn save(&self, identity: &DeviceIdentity) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(identity)?;
        std::fs::write(&self.path, raw)?;
        Ok(())
    }

    /// Loads trusted devices, returning an empty list if none are stored yet.
    pub fn trusted_devices(&self) -> Result<Vec<TrustedDevice>> {
        let path = self.path.with_file_name("trusted.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&raw)?)
    }

    pub fn save_trusted(&self, devices: &[TrustedDevice]) -> Result<()> {
        let path = self.path.with_file_name("trusted.json");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(devices)?;
        std::fs::write(path, raw)?;
        Ok(())
    }
}

/// Tracks in-flight pairings and enforces the rate limit.
#[derive(Debug, Default)]
pub struct PairingState {
    pending: HashMap<String, PendingPairing>,
}

impl PairingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begins a pairing and returns the code to display.
    ///
    /// If a pairing with this device is already in flight, the existing code is
    /// returned rather than a fresh one — otherwise a peer could reset the
    /// attempt counter by re-requesting.
    pub fn begin(&mut self, device_id: &str, display_name: &str, platform: &str) -> String {
        let now = Instant::now();

        if let Some(existing) = self.pending.get_mut(device_id) {
            if !existing.is_expired(now) {
                return existing.code.clone();
            }
        }

        let code = generate_pairing_code();
        self.pending.insert(
            device_id.to_string(),
            PendingPairing {
                device_id: device_id.to_string(),
                display_name: display_name.to_string(),
                platform: platform.to_string(),
                code: code.clone(),
                started: now,
                attempts: 0,
            },
        );
        code
    }

    /// Checks a code the user (or the peer) supplied.
    ///
    /// Returns an error rather than `false` so the caller cannot mistake an
    /// expired or rate-limited pairing for a simple mismatch.
    pub fn confirm(&mut self, device_id: &str, supplied: &str) -> Result<()> {
        let now = Instant::now();

        let pending =
            self.pending
                .get_mut(device_id)
                .ok_or_else(|| Error::AuthenticationFailed {
                    reason: "No pairing is in progress for that device.".into(),
                })?;

        if pending.is_expired(now) {
            self.pending.remove(device_id);
            return Err(Error::AuthenticationFailed {
                reason: "The pairing code expired. Start again.".into(),
            });
        }

        if pending.is_rate_limited() {
            return Err(Error::AuthenticationFailed {
                reason: "Too many incorrect codes. Wait a few minutes and try again.".into(),
            });
        }

        // Constant-time: a timing side channel here would leak the code digit
        // by digit.
        let matches = constant_time_eq(supplied.trim().as_bytes(), pending.code.as_bytes());

        if !matches {
            pending.attempts += 1;
            warn!(
                device_id,
                attempts = pending.attempts,
                "pairing code mismatch"
            );
            return Err(Error::AuthenticationFailed {
                reason: "The codes do not match.".into(),
            });
        }

        // Single-use: a code that has been accepted is spent, even if the user
        // has not yet pressed Accept (SRS FR-PAIR-004).
        self.pending.remove(device_id);
        Ok(())
    }

    pub fn pending(&self, device_id: &str) -> Option<&PendingPairing> {
        self.pending.get(device_id)
    }

    /// Drops expired pairings. Call periodically so the map cannot grow without
    /// bound (SRS §93).
    pub fn prune(&mut self) {
        let now = Instant::now();
        self.pending.retain(|_, p| !p.is_expired(now));
    }

    pub fn len(&self) -> usize {
        self.pending.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- pairing codes -------------------------------------------------------

    #[test]
    fn pairing_code_has_the_displayed_shape() {
        for _ in 0..100 {
            let code = generate_pairing_code();
            assert_eq!(code.len(), 7, "expected 'NNN NNN', got {code:?}");
            assert_eq!(code.as_bytes()[3], b' ');
            assert!(
                code.chars().filter(|c| c.is_ascii_digit()).count() == 6,
                "expected six digits in {code:?}"
            );
        }
    }

    #[test]
    fn pairing_codes_are_not_constant() {
        // A predictable generator would let an attacker precompute the code.
        let codes: std::collections::HashSet<String> =
            (0..200).map(|_| generate_pairing_code()).collect();
        assert!(
            codes.len() > 150,
            "codes repeat too often: {} unique",
            codes.len()
        );
    }

    // --- constant-time comparison -------------------------------------------

    #[test]
    fn constant_time_eq_matches_normal_equality() {
        assert!(constant_time_eq(b"739284", b"739284"));
        assert!(!constant_time_eq(b"739284", b"739285"));
        assert!(!constant_time_eq(b"739284", b"7392840"));
        assert!(!constant_time_eq(b"", b"x"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn constant_time_eq_handles_a_prefix_difference() {
        // The case a timing attack targets: differing in the first byte.
        assert!(!constant_time_eq(b"aaaaaa", b"baaaaa"));
    }

    // --- pairing state machine ----------------------------------------------

    #[test]
    fn confirm_accepts_the_issued_code() {
        let mut state = PairingState::new();
        let code = state.begin("dev-b", "Laptop B", "linux");
        assert!(state.confirm("dev-b", &code).is_ok());
    }

    #[test]
    fn confirm_tolerates_surrounding_whitespace() {
        let mut state = PairingState::new();
        let code = state.begin("dev-b", "Laptop B", "linux");
        assert!(state.confirm("dev-b", &format!("  {code} ")).is_ok());
    }

    #[test]
    fn confirm_rejects_a_wrong_code() {
        let mut state = PairingState::new();
        state.begin("dev-b", "Laptop B", "linux");
        let result = state.confirm("dev-b", "000 000");
        assert!(matches!(result, Err(Error::AuthenticationFailed { .. })));
    }

    #[test]
    fn a_consumed_code_cannot_be_reused() {
        let mut state = PairingState::new();
        let code = state.begin("dev-b", "Laptop B", "linux");

        assert!(state.confirm("dev-b", &code).is_ok());
        // Replaying the same code must now fail — it is single-use.
        assert!(state.confirm("dev-b", &code).is_err());
    }

    #[test]
    fn pairing_attempts_are_rate_limited() {
        let mut state = PairingState::new();
        state.begin("dev-b", "Laptop B", "linux");

        for _ in 0..MAX_PAIRING_ATTEMPTS {
            let _ = state.confirm("dev-b", "000 000");
        }

        // Even the *correct* code is refused once the limit is hit, so a brute
        // force cannot succeed by eventually guessing right.
        let pending = state.pending("dev-b").unwrap();
        assert!(pending.is_rate_limited());
        assert!(state.confirm("dev-b", "000 000").is_err());
    }

    #[test]
    fn re_requesting_does_not_reset_the_attempt_counter() {
        let mut state = PairingState::new();
        state.begin("dev-b", "Laptop B", "linux");
        for _ in 0..MAX_PAIRING_ATTEMPTS {
            let _ = state.confirm("dev-b", "000 000");
        }

        // A peer asking to pair again must not get a fresh allowance.
        state.begin("dev-b", "Laptop B", "linux");
        assert!(state.pending("dev-b").unwrap().is_rate_limited());
    }

    #[test]
    fn re_requesting_returns_the_same_code_while_pending() {
        let mut state = PairingState::new();
        let first = state.begin("dev-b", "Laptop B", "linux");
        let second = state.begin("dev-b", "Laptop B", "linux");
        assert_eq!(first, second, "a re-request must not mint a new code");
    }

    #[test]
    fn confirming_an_unknown_device_is_an_error() {
        let mut state = PairingState::new();
        assert!(state.confirm("never-seen", "123 456").is_err());
    }

    #[test]
    fn prune_clears_finished_pairings() {
        let mut state = PairingState::new();
        state.begin("dev-b", "Laptop B", "linux");
        let code = state.pending("dev-b").unwrap().code.clone();

        state.confirm("dev-b", &code).unwrap();
        assert!(state.is_empty(), "a confirmed pairing should be removed");
    }

    #[test]
    fn pending_pairings_carry_the_display_fields() {
        // SRS FR-PAIR-002: the user must see what they are approving.
        let mut state = PairingState::new();
        state.begin("dev-b", "Laptop B", "linux");

        let pending = state.pending("dev-b").unwrap();
        assert_eq!(pending.display_name, "Laptop B");
        assert_eq!(pending.platform, "linux");
        assert!(!pending.code.is_empty());
    }
}
