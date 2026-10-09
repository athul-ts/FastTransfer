//! Wire protocol: framing, message types, and version negotiation.
//!
//! Implements SRS §18.4 (framing), §19 (session establishment), §58 (versioning),
//! and §59 (capability negotiation).
//!
//! # Framing
//!
//! Every message is a 4-byte big-endian length followed by that many bytes of
//! JSON:
//!
//! ```text
//! +----------------+---------------------------+
//! | len: u32 (BE)  | payload: len bytes (JSON) |
//! +----------------+---------------------------+
//! ```
//!
//! The length is validated against [`MAX_MESSAGE_SIZE`] **before** any buffer is
//! allocated. A peer that advertises a 4 GiB frame gets an error, not an OOM —
//! see SRS §93.
//!
//! # Trust
//!
//! Everything decoded here comes from a peer that may be hostile. Decoding
//! produces typed values; it does not validate them. Bounds checks on sizes,
//! indices, and paths belong to the consumer (SRS §92).

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::{Error, Result};

/// Wire protocol version. Bump the minor for additive changes, the major for
/// breaking ones. Negotiation takes the lower of the two peers' versions.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 0 };

/// Hard ceiling on a single message. Chosen so the largest legitimate message —
/// a capability blob or a chunk manifest — fits comfortably while a hostile
/// length cannot exhaust memory.
pub const MAX_MESSAGE_SIZE: u32 = 16 * 1024 * 1024;

/// Default chunk size. Negotiated down at session start if the peer wants less.
pub const DEFAULT_CHUNK_SIZE: u32 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    /// Negotiation takes the lower version on each axis. A `2.x` peer talking to
    /// a `1.y` peer agrees on `1`; if the majors differ the session is refused
    /// rather than silently degraded (SRS §58).
    pub fn negotiate(self, other: Self) -> Result<Self> {
        if self.major != other.major {
            return Err(Error::VersionMismatch {
                local: self.to_string(),
                remote: other.to_string(),
            });
        }
        Ok(Self {
            major: self.major,
            minor: self.minor.min(other.minor),
        })
    }
}

impl std::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// What each endpoint can do. Both sides send one in `Hello`; the intersection
/// is the session's feature set (SRS §59).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub protocol: ProtocolVersion,
    pub max_chunk_size: u32,
    pub resume: bool,
    pub sha256: bool,
    pub compression: bool,
    pub encryption: bool,
    /// Transport kinds this endpoint can serve, most preferred first.
    pub transports: Vec<String>,
}

impl Default for Capabilities {
    fn default() -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            max_chunk_size: DEFAULT_CHUNK_SIZE,
            resume: true,
            sha256: true,
            // Compression stays off until §60's detector exists; enabling it
            // blindly burns CPU on already-compressed data.
            compression: false,
            encryption: false,
            transports: vec!["thunderbolt".into(), "ethernet".into(), "wifi".into()],
        }
    }
}

impl Capabilities {
    /// Features both peers support. Anything not in the intersection is off for
    /// this session.
    pub fn intersect(&self, other: &Self) -> Result<Self> {
        Ok(Self {
            protocol: self.protocol.negotiate(other.protocol)?,
            max_chunk_size: self.max_chunk_size.min(other.max_chunk_size),
            resume: self.resume && other.resume,
            sha256: self.sha256 && other.sha256,
            compression: self.compression && other.compression,
            encryption: self.encryption && other.encryption,
            transports: self
                .transports
                .iter()
                .filter(|t| other.transports.contains(t))
                .cloned()
                .collect(),
        })
    }
}

/// A single file or directory in a transfer manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// Path relative to the transfer root. **Untrusted** — the receiver must
    /// pass this through `filesystem::safe_join` before use (SRS §91).
    pub relative_path: String,
    pub size: u64,
    pub is_dir: bool,
    /// Unix mode where meaningful; ignored where it is not.
    pub mode: Option<u32>,
    /// Modification time, seconds since the Unix epoch.
    pub modified: Option<i64>,
    /// SHA-256 of the file. Hex, lowercase. Absent for directories.
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// First message on a connection.
    Hello {
        device_id: String,
        display_name: String,
        app_version: String,
        platform: String,
        capabilities: Capabilities,
        /// Random per-connection nonce, echoed in `HelloAck` to bind the reply
        /// to this attempt and reject replayed handshakes.
        nonce: [u8; 32],
    },

    HelloAck {
        device_id: String,
        display_name: String,
        app_version: String,
        platform: String,
        capabilities: Capabilities,
        nonce: [u8; 32],
        /// The nonce from the peer's `Hello`, echoed back.
        peer_nonce: [u8; 32],
    },

    /// Pairing proof. Empty until the Phase 7 security work lands.
    Authenticate {
        device_id: String,
        proof: Vec<u8>,
    },

    AuthOk {
        session_id: String,
    },

    AuthFailed {
        reason: String,
    },

    /// Sender proposes a transfer.
    TransferRequest {
        transfer_id: String,
        total_bytes: u64,
        file_count: u64,
        entries: Vec<FileEntry>,
    },

    Accept {
        transfer_id: String,
        /// Chunks the receiver already holds, for resume (SRS §22).
        completed_chunks: Vec<ChunkRef>,
    },

    Reject {
        transfer_id: String,
        reason: String,
    },

    /// A resume manifest request, sent when the sender believes a transfer is
    /// partially complete and wants the receiver's ground truth.
    ResumeQuery {
        transfer_id: String,
    },

    ResumeManifest {
        transfer_id: String,
        completed_chunks: Vec<ChunkRef>,
    },

    ChunkHeader {
        transfer_id: String,
        file_index: u32,
        chunk_index: u64,
        size: u32,
    },

    /// Terminates a chunk's data. The receiver treats a chunk as complete only
    /// once it has read `size` bytes *and* the trailing digest matches.
    ChunkFooter {
        transfer_id: String,
        file_index: u32,
        chunk_index: u64,
        sha256: [u8; 32],
    },

    TransferComplete {
        transfer_id: String,
        /// Per-file digests so the receiver can verify end to end.
        digests: Vec<FileDigest>,
    },

    /// Receiver confirms it verified everything.
    Verified {
        transfer_id: String,
    },

    /// Receiver found a digest mismatch. Marks the transfer failed, never complete.
    VerificationFailed {
        transfer_id: String,
        relative_path: String,
        expected: String,
        actual: String,
    },

    Error {
        transfer_id: Option<String>,
        code: String,
        detail: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChunkRef {
    pub file_index: u32,
    pub chunk_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileDigest {
    pub file_index: u32,
    pub sha256: String,
}

impl Message {
    /// The transfer this message belongs to, when it belongs to one. Used to
    /// route messages to the right session without inspecting the variant at
    /// every call site.
    pub fn transfer_id(&self) -> Option<&str> {
        match self {
            Message::TransferRequest { transfer_id, .. }
            | Message::Accept { transfer_id, .. }
            | Message::Reject { transfer_id, .. }
            | Message::ResumeQuery { transfer_id }
            | Message::ResumeManifest { transfer_id, .. }
            | Message::ChunkHeader { transfer_id, .. }
            | Message::ChunkFooter { transfer_id, .. }
            | Message::TransferComplete { transfer_id, .. }
            | Message::Verified { transfer_id }
            | Message::VerificationFailed { transfer_id, .. }
            | Message::Error {
                transfer_id: Some(transfer_id),
                ..
            } => Some(transfer_id),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Framing
// ---------------------------------------------------------------------------

/// Writes one length-prefixed message.
///
/// The payload is serialized first so the length prefix is always truthful — we
/// never advertise a size we cannot then deliver.
pub async fn write_message<W>(writer: &mut W, msg: &Message) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(msg)?;

    let len = u32::try_from(payload.len()).map_err(|_| Error::MessageTooLarge {
        size: payload.len() as u64,
        limit: MAX_MESSAGE_SIZE,
    })?;

    if len > MAX_MESSAGE_SIZE {
        return Err(Error::MessageTooLarge {
            size: len as u64,
            limit: MAX_MESSAGE_SIZE,
        });
    }

    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads one length-prefixed message.
///
/// The length is checked against [`MAX_MESSAGE_SIZE`] *before* the buffer is
/// allocated, so a peer claiming a 4 GiB frame costs us nothing. A declared
/// length that the peer then fails to deliver surfaces as a clean `UnexpectedEof`
/// rather than a hang (SRS §90, §93).
pub async fn read_message<R>(reader: &mut R) -> Result<Message>
where
    R: AsyncRead + Unpin,
{
    let mut header = [0u8; 4];
    reader.read_exact(&mut header).await?;
    let len = u32::from_be_bytes(header);

    if len > MAX_MESSAGE_SIZE {
        return Err(Error::MessageTooLarge {
            size: len as u64,
            limit: MAX_MESSAGE_SIZE,
        });
    }

    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;

    serde_json::from_slice(&buf).map_err(|e| Error::MalformedMessage(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    fn hello(nonce_byte: u8) -> Message {
        Message::Hello {
            device_id: "dev-a".into(),
            display_name: "Laptop A".into(),
            app_version: "0.1.0".into(),
            platform: "windows".into(),
            capabilities: Capabilities::default(),
            nonce: [nonce_byte; 32],
        }
    }

    #[tokio::test]
    async fn round_trip_preserves_message() {
        let original = hello(7);
        let mut wire = Vec::new();
        write_message(&mut wire, &original).await.unwrap();

        let mut reader = BufReader::new(wire.as_slice());
        let decoded = read_message(&mut reader).await.unwrap();
        assert_eq!(original, decoded);
    }

    #[tokio::test]
    async fn oversized_length_is_rejected_before_reading_body() {
        // A header promising 4 GiB, with no body at all. Must fail on the
        // header — if this test hangs or OOMs, the guard has regressed.
        let mut wire = Vec::new();
        wire.extend_from_slice(&u32::MAX.to_be_bytes());

        let mut reader = BufReader::new(wire.as_slice());
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(
            matches!(err, Error::MessageTooLarge { .. }),
            "expected MessageTooLarge, got {err:?}"
        );
    }

    #[tokio::test]
    async fn truncated_body_errors_cleanly() {
        // Header promises 100 bytes; only 10 arrive.
        let mut wire = Vec::new();
        wire.extend_from_slice(&100u32.to_be_bytes());
        wire.extend_from_slice(&[0u8; 10]);

        let mut reader = BufReader::new(wire.as_slice());
        let err = read_message(&mut reader).await.unwrap_err();
        assert!(matches!(err, Error::Io(_)), "expected Io, got {err:?}");
    }

    #[test]
    fn negotiation_takes_the_lower_minor() {
        let a = ProtocolVersion { major: 1, minor: 2 };
        let b = ProtocolVersion { major: 1, minor: 1 };
        assert_eq!(
            a.negotiate(b).unwrap(),
            ProtocolVersion { major: 1, minor: 1 }
        );
    }

    #[test]
    fn negotiation_refuses_different_majors() {
        let a = ProtocolVersion { major: 2, minor: 0 };
        let b = ProtocolVersion { major: 1, minor: 9 };
        assert!(matches!(a.negotiate(b), Err(Error::VersionMismatch { .. })));
    }

    #[test]
    fn capability_intersection_drops_unsupported_features() {
        let a = Capabilities::default();
        let b = Capabilities {
            compression: true,
            resume: false,
            transports: vec!["ethernet".into()],
            ..Capabilities::default()
        };
        let agreed = a.intersect(&b).unwrap();
        assert!(!agreed.compression, "one side lacks compression");
        assert!(!agreed.resume, "one side lacks resume");
        assert_eq!(agreed.transports, vec!["ethernet".to_string()]);
    }

    #[test]
    fn chunk_size_negotiates_down_to_the_smaller_peer() {
        let a = Capabilities {
            max_chunk_size: 8 * 1024 * 1024,
            ..Capabilities::default()
        };
        let b = Capabilities {
            max_chunk_size: 1024 * 1024,
            ..Capabilities::default()
        };
        assert_eq!(a.intersect(&b).unwrap().max_chunk_size, 1024 * 1024);
    }
}
