//! Error types for the FastTransfer core.
//!
//! Every variant is shaped to answer the three questions the UI must always be
//! able to render (SRS §95): what happened, why, and what the user can do next.
//! [`Error::classify`] produces the payload the frontend receives.
//!
//! Errors are mapped to user-facing meaning *here*, not at the call site — so
//! the mapping stays consistent and a raw `ECONNRESET` can never reach the UI
//! by accident.

use serde::Serialize;

use crate::link::LinkKind;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A cable is present but cannot carry a transfer. SRS FR-LINK-007 requires
    /// this to explain itself rather than fail opaquely.
    #[error("link is not usable for transfer: {kind:?}")]
    LinkUnusable {
        kind: LinkKind,
        what: String,
        remedy: String,
    },

    #[error("no usable transport found")]
    NoTransport,

    #[error("peer not found on any local link")]
    PeerNotFound,

    #[error("connection lost after {transferred} bytes")]
    ConnectionLost { transferred: u64, resumable: bool },

    #[error("peer rejected the transfer")]
    PeerRejected,

    #[error("authentication failed: {reason}")]
    AuthenticationFailed { reason: String },

    #[error("protocol version mismatch: local {local}, remote {remote}")]
    VersionMismatch { local: String, remote: String },

    #[error("malformed protocol message: {0}")]
    MalformedMessage(String),

    #[error("message of {size} bytes exceeds the {limit} byte limit")]
    MessageTooLarge { size: u64, limit: u32 },

    /// Path traversal attempt, or a path that resolves outside the destination.
    /// This is a security event, not an ordinary I/O failure — SRS §91.
    #[error("path escapes the destination directory: {0}")]
    UnsafePath(String),

    #[error("destination is not writable: {path}")]
    DestinationNotWritable { path: String, remedy: String },

    #[error("destination disk is full: {required} bytes required, {available} bytes free")]
    DiskFull { required: u64, available: u64 },

    #[error("integrity check failed for {path}: expected {expected}, got {actual}")]
    VerificationFailed {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("transfer not found: {0}")]
    TransferNotFound(String),

    #[error("operation cancelled")]
    Cancelled,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// The structured payload the frontend renders. Mirrors `AppError` in
/// `src/types/index.ts` — keep them in step.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    /// Stable machine-readable tag the UI can branch on.
    pub kind: &'static str,
    /// One line: what happened.
    pub what: String,
    /// Why it happened, when we know.
    pub why: Option<String>,
    /// What the user can do. `None` only when there is genuinely nothing.
    pub remedy: Option<String>,
    /// Whether a Resume affordance is valid for this failure.
    pub resumable: bool,
}

impl Error {
    pub fn classify(&self) -> ErrorPayload {
        // The `kind` strings are part of the IPC contract; the frontend switches
        // on them. Changing one is a breaking change — update src/types/index.ts.
        match self {
            Error::LinkUnusable { what, remedy, .. } => ErrorPayload {
                kind: "linkUnusable",
                what: what.clone(),
                why: Some(self.to_string()),
                remedy: Some(remedy.clone()),
                resumable: false,
            },
            Error::NoTransport => ErrorPayload {
                kind: "noTransport",
                what: "No usable connection found.".into(),
                why: Some("Neither a Thunderbolt cable, Ethernet, nor Wi-Fi link is available.".into()),
                remedy: Some("Connect a Thunderbolt cable, or join a network.".into()),
                resumable: false,
            },
            Error::PeerNotFound => ErrorPayload {
                kind: "peerNotFound",
                what: "The other computer was not found.".into(),
                why: Some("FastTransfer is not running there, or the link is blocked by a firewall.".into()),
                remedy: Some("Check that FastTransfer is open on both computers, then retry.".into()),
                resumable: true,
            },
            Error::ConnectionLost { resumable, .. } => ErrorPayload {
                kind: "connectionLost",
                what: "Connection interrupted.".into(),
                why: Some("The cable was disconnected, or the destination is no longer reachable.".into()),
                remedy: Some("Reconnect and resume — completed data is kept.".into()),
                resumable: *resumable,
            },
            Error::PeerRejected => ErrorPayload {
                kind: "peerRejected",
                what: "The other computer declined the transfer.".into(),
                why: None,
                remedy: Some("Ask the person at the other computer to accept.".into()),
                resumable: false,
            },
            Error::AuthenticationFailed { reason } => ErrorPayload {
                kind: "authenticationFailed",
                what: "Could not verify the other computer.".into(),
                why: Some(reason.clone()),
                remedy: Some("Check that the verification codes match on both screens.".into()),
                resumable: false,
            },
            Error::VersionMismatch { local, remote } => ErrorPayload {
                kind: "versionMismatch",
                what: "The two computers are running incompatible versions.".into(),
                why: Some(format!("This computer speaks {local}; the other speaks {remote}.")),
                remedy: Some("Update FastTransfer on both computers.".into()),
                resumable: false,
            },
            Error::MalformedMessage(_) | Error::MessageTooLarge { .. } => ErrorPayload {
                kind: "protocolError",
                what: "The connection sent data FastTransfer could not understand.".into(),
                why: Some(self.to_string()),
                remedy: Some("Retry. If it repeats, the other computer may be running an incompatible build.".into()),
                resumable: false,
            },
            // A traversal attempt is reported as a refusal, never as a silent
            // success or a partial write.
            Error::UnsafePath(p) => ErrorPayload {
                kind: "unsafePath",
                what: "A file was refused because it would be written outside the destination folder.".into(),
                why: Some(format!("Rejected path: {p}")),
                remedy: Some("This is a safety check. Nothing was written. Report it if it repeats.".into()),
                resumable: false,
            },
            Error::DestinationNotWritable { path, remedy } => ErrorPayload {
                kind: "destinationNotWritable",
                what: "The destination folder cannot be written to.".into(),
                why: Some(format!("{path} is not writable with current permissions.")),
                remedy: Some(remedy.clone()),
                resumable: false,
            },
            Error::DiskFull { required, available } => ErrorPayload {
                kind: "diskFull",
                what: "The destination disk is full.".into(),
                why: Some(format!(
                    "{} required, {} free.",
                    human_bytes(*required),
                    human_bytes(*available)
                )),
                remedy: Some("Choose another folder, or free space and retry.".into()),
                resumable: true,
            },
            Error::VerificationFailed { path, .. } => ErrorPayload {
                kind: "verificationFailed",
                what: "The file did not survive the transfer intact.".into(),
                why: Some(format!("SHA-256 mismatch on {path}.")),
                remedy: Some("Retry the transfer. The file has been marked failed, not complete.".into()),
                resumable: true,
            },
            Error::TransferNotFound(id) => ErrorPayload {
                kind: "transferNotFound",
                what: "That transfer no longer exists.".into(),
                why: Some(format!("Unknown transfer id: {id}")),
                remedy: None,
                resumable: false,
            },
            Error::Cancelled => ErrorPayload {
                kind: "cancelled",
                what: "Cancelled.".into(),
                why: None,
                remedy: None,
                resumable: false,
            },
            Error::Io(e) => ErrorPayload {
                kind: "io",
                what: "A file operation failed.".into(),
                why: Some(e.to_string()),
                remedy: Some("Check that the destination still exists and retry.".into()),
                resumable: true,
            },
            Error::Serde(e) => ErrorPayload {
                kind: "protocolError",
                what: "The connection sent malformed data.".into(),
                why: Some(e.to_string()),
                remedy: Some("Retry.".into()),
                resumable: false,
            },
            Error::Other(e) => ErrorPayload {
                kind: "internal",
                what: "Something went wrong inside FastTransfer.".into(),
                why: Some(e.to_string()),
                remedy: Some("Retry. If it persists, check the logs in Settings > Advanced.".into()),
                resumable: false,
            },
        }
    }
}

/// Serialize as the structured payload so the frontend never sees a raw Debug
/// string. Tauri's `Result<T, E>` requires `E: Serialize`.
impl Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        self.classify().serialize(s)
    }
}

/// Formats a byte count for display in an error. Deliberately duplicated from
/// the frontend formatter rather than shared — errors must render even if the
/// UI's formatting module fails to load.
fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_full_error_reports_resumable() {
        let err = Error::DiskFull {
            required: 80_000_000_000,
            available: 12_000_000_000,
        };
        let payload = err.classify();
        assert!(payload.resumable, "disk-full must offer resume");
        assert_eq!(payload.kind, "diskFull");
    }

    #[test]
    fn unsafe_path_is_never_resumable() {
        let payload = Error::UnsafePath("../etc/passwd".into()).classify();
        assert!(!payload.resumable);
        assert_eq!(payload.kind, "unsafePath");
    }

    #[test]
    fn every_payload_answers_what_happened() {
        // SRS §95: "what happened" is mandatory for every error.
        let samples = vec![
            Error::NoTransport,
            Error::PeerNotFound,
            Error::PeerRejected,
            Error::Cancelled,
            Error::TransferNotFound("abc".into()),
        ];
        for err in samples {
            assert!(
                !err.classify().what.is_empty(),
                "error {err:?} produced an empty 'what'"
            );
        }
    }
}
