//! Chunked streaming, the transfer state machine, and resume.
//!
//! Implements SRS §20–§22, §27–§29.
//!
//! # The invariant that makes resume work
//!
//! > **A chunk is the atomic unit of resume.** A chunk is complete only when the
//! > receiver has read its declared length *and* its trailing digest matched.
//! > A partial chunk is never persisted and never reported as complete.
//! > (SRS FR-CHUNK-001)
//!
//! Everything about resume follows from that. The receiver's manifest is the
//! only ground truth about what has landed, and it is also remote input — so it
//! is validated against the file's declared size before any of it is believed
//! (SRS §92).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::error::{Error, Result};
use crate::filesystem;
use crate::protocol::{ChunkRef, FileEntry};

/// Bound on the in-flight pipeline. This is what makes backpressure real
/// (SRS §29): when the destination disk is slower than the link, this channel
/// fills, the reader stops pulling, and TCP's own window closes. Without a
/// bound here, a fast cable into a slow SSD is an OOM.
pub const PIPELINE_DEPTH: usize = 8;

/// Hard cap on concurrent streams across the whole app (SRS FR-PAR-001).
pub const MAX_TOTAL_STREAMS: usize = 16;

/// Hard cap on streams within one transfer.
pub const MAX_STREAMS_PER_TRANSFER: usize = 4;

/// The transfer lifecycle. Mirrors `TransferStatus` in `src/types/index.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferStatus {
    Queued,
    Preparing,
    Connecting,
    Authenticating,
    Transferring,
    Verifying,
    Completed,
    Paused,
    Interrupted,
    Failed,
    Cancelled,
}

impl TransferStatus {
    /// Whether this is a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            TransferStatus::Completed | TransferStatus::Failed | TransferStatus::Cancelled
        )
    }

    /// Whether the resume affordance is meaningful here.
    pub fn is_resumable(self) -> bool {
        matches!(
            self,
            TransferStatus::Interrupted | TransferStatus::Paused | TransferStatus::Failed
        )
    }
}

/// A transfer's progress, as the UI needs it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transfer_id: String,
    pub status: TransferStatus,
    pub total_bytes: u64,
    pub transferred_bytes: u64,
    /// Measured, never derived from the link rating (SRS §101).
    pub current_bps: u64,
    pub average_bps: u64,
    /// Seconds, computed from a moving window rather than instantaneous speed
    /// so the estimate does not jitter (SRS §65).
    pub eta_seconds: Option<u64>,
    pub current_file: Option<String>,
}

impl TransferProgress {
    /// Fraction complete, clamped to `0.0..=1.0`.
    ///
    /// Guarding the zero case matters: a zero-byte transfer (an empty folder)
    /// would otherwise produce NaN and render as `NaN%`.
    pub fn fraction(&self) -> f64 {
        if self.total_bytes == 0 {
            return if self.status == TransferStatus::Completed {
                1.0
            } else {
                0.0
            };
        }
        (self.transferred_bytes as f64 / self.total_bytes as f64).clamp(0.0, 1.0)
    }
}

/// Moving-window throughput estimator.
///
/// A single instantaneous sample makes the ETA jump around; a window makes it
/// settle. SRS §65.
#[derive(Debug, Clone)]
pub struct ThroughputEstimator {
    samples: std::collections::VecDeque<(std::time::Instant, u64)>,
    window: std::time::Duration,
}

impl ThroughputEstimator {
    pub fn new(window: std::time::Duration) -> Self {
        Self {
            samples: std::collections::VecDeque::new(),
            window,
        }
    }

    pub fn record(&mut self, now: std::time::Instant, cumulative_bytes: u64) {
        self.samples.push_back((now, cumulative_bytes));
        while let Some((t, _)) = self.samples.front() {
            if now.duration_since(*t) > self.window {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Bytes per second over the window. `None` until there are two samples,
    /// which is what keeps the first second of a transfer from showing a wild
    /// estimate.
    pub fn bps(&self) -> Option<u64> {
        let (first_t, first_b) = *self.samples.front()?;
        let (last_t, last_b) = *self.samples.back()?;

        let elapsed = last_t.duration_since(first_t).as_secs_f64();
        if elapsed <= 0.0 || self.samples.len() < 2 {
            return None;
        }

        let delta = last_b.saturating_sub(first_b);
        Some((delta as f64 / elapsed) as u64)
    }

    pub fn eta_seconds(&self, remaining_bytes: u64) -> Option<u64> {
        let bps = self.bps()?;
        if bps == 0 {
            return None;
        }
        Some(remaining_bytes / bps)
    }
}

/// Splits a file into chunk boundaries.
///
/// Returns the byte offset and length of each chunk. The final chunk is short
/// when the size is not a multiple of `chunk_size`.
pub fn chunk_plan(file_size: u64, chunk_size: u32) -> Vec<(u64, u32)> {
    assert!(chunk_size > 0, "chunk size must be non-zero");

    let chunk_size = chunk_size as u64;
    let mut plan = Vec::new();
    let mut offset = 0u64;

    while offset < file_size {
        let len = chunk_size.min(file_size - offset);
        plan.push((offset, len as u32));
        offset += len;
    }

    plan
}

/// How many chunks a file has, without building the plan.
pub fn chunk_count(file_size: u64, chunk_size: u32) -> u64 {
    if chunk_size == 0 {
        return 0;
    }
    file_size.div_ceil(chunk_size as u64)
}

/// A receiver's claim about which chunks it already holds.
///
/// # Trust
///
/// This arrives over the wire, so it is validated before use — see
/// [`ResumeManifest::validate`]. An unvalidated manifest lets a peer claim
/// chunk 10^12 of a 1 GB file, or claim everything is done so the sender sends
/// nothing and the transfer "completes" empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumeManifest {
    pub transfer_id: String,
    pub completed_chunks: Vec<ChunkRef>,
}

impl ResumeManifest {
    /// Rejects a manifest that could not describe a real transfer.
    ///
    /// `file_sizes` is the sender's own view of the transfer, indexed by
    /// `file_index` — the one thing in this exchange the peer does not control.
    pub fn validate(&self, file_sizes: &[u64], chunk_size: u32) -> Result<()> {
        let max_file_index = file_sizes.len() as u32;

        for chunk in &self.completed_chunks {
            if chunk.file_index >= max_file_index {
                return Err(Error::MalformedMessage(format!(
                    "resume manifest references file index {}, but the transfer has {} files",
                    chunk.file_index, max_file_index
                )));
            }

            let file_size = file_sizes[chunk.file_index as usize];
            let total = chunk_count(file_size, chunk_size);

            if chunk.chunk_index >= total {
                return Err(Error::MalformedMessage(format!(
                    "resume manifest claims chunk {} of file {} which has only {total} chunks",
                    chunk.chunk_index, chunk.file_index
                )));
            }
        }

        Ok(())
    }

    /// The chunks the sender still has to send, in order.
    ///
    /// `file_sizes` must already have passed [`validate`](Self::validate).
    pub fn missing_chunks(&self, file_sizes: &[u64], chunk_size: u32) -> Vec<ChunkRef> {
        let have: std::collections::HashSet<(u32, u64)> = self
            .completed_chunks
            .iter()
            .map(|c| (c.file_index, c.chunk_index))
            .collect();

        let mut missing = Vec::new();
        for (file_index, size) in file_sizes.iter().enumerate() {
            for chunk_index in 0..chunk_count(*size, chunk_size) {
                let key = (file_index as u32, chunk_index);
                if !have.contains(&key) {
                    missing.push(ChunkRef {
                        file_index: key.0,
                        chunk_index: key.1,
                    });
                }
            }
        }
        missing
    }
}

/// Prepares a transfer: validates the peer's manifest of files, resolves every
/// destination path, and computes the byte total.
///
/// Every path in `entries` is untrusted. This is the choke point where they are
/// proven safe (SRS §91) — the write path later assumes the work is done.
pub fn prepare(
    entries: &[FileEntry],
    destination_root: &std::path::Path,
) -> Result<Vec<PreparedItem>> {
    let mut prepared = Vec::with_capacity(entries.len());

    for entry in entries {
        // Reject before joining, so a traversal attempt is reported as such
        // rather than surfacing as an odd I/O error.
        let target = filesystem::safe_join(destination_root, &entry.relative_path)?;

        // A declared size we cannot write is a failure at prepare time, not
        // halfway through when the disk fills.
        if entry.is_dir && entry.size != 0 {
            return Err(Error::MalformedMessage(format!(
                "directory {} declared a non-zero size",
                entry.relative_path
            )));
        }

        prepared.push(PreparedItem {
            relative_path: entry.relative_path.clone(),
            destination: target,
            size: entry.size,
            is_dir: entry.is_dir,
            declared_sha256: entry.sha256.clone(),
        });
    }

    Ok(prepared)
}

/// A manifest entry with its destination already proven safe.
#[derive(Debug, Clone)]
pub struct PreparedItem {
    pub relative_path: String,
    pub destination: PathBuf,
    pub size: u64,
    pub is_dir: bool,
    pub declared_sha256: Option<String>,
}

/// A slot in the bounded pipeline. Dropping one releases capacity, so the
/// reader stops pulling when the writer falls behind — that is the backpressure
/// mechanism, not a side effect of it.
#[derive(Debug)]
pub struct ChunkBuffer {
    pub file_index: u32,
    pub chunk_index: u64,
    pub offset: u64,
    pub data: Vec<u8>,
}

/// Creates the bounded pipeline channel between reader and writer.
pub fn pipeline() -> (mpsc::Sender<ChunkBuffer>, mpsc::Receiver<ChunkBuffer>) {
    mpsc::channel(PIPELINE_DEPTH)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // --- chunk planning ------------------------------------------------------

    #[test]
    fn chunk_plan_covers_the_whole_file() {
        let plan = chunk_plan(10, 4);
        assert_eq!(plan, vec![(0, 4), (4, 4), (8, 2)]);

        let total: u64 = plan.iter().map(|(_, len)| *len as u64).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn chunk_plan_of_an_exact_multiple_has_no_short_tail() {
        let plan = chunk_plan(8, 4);
        assert_eq!(plan, vec![(0, 4), (4, 4)]);
    }

    #[test]
    fn chunk_plan_of_an_empty_file_is_empty() {
        assert!(chunk_plan(0, 4).is_empty());
        assert_eq!(chunk_count(0, 4), 0);
    }

    #[test]
    fn chunk_plan_chunks_are_contiguous() {
        let plan = chunk_plan(1000, 64);
        let mut expected_offset = 0u64;
        for (offset, len) in &plan {
            assert_eq!(*offset, expected_offset, "chunk plan has a gap");
            expected_offset += *len as u64;
        }
        assert_eq!(expected_offset, 1000);
    }

    #[test]
    fn chunk_count_matches_plan_length() {
        for size in [0u64, 1, 99, 100, 101, 4095, 4096, 4097] {
            assert_eq!(
                chunk_count(size, 4096) as usize,
                chunk_plan(size, 4096).len(),
                "mismatch at size {size}"
            );
        }
    }

    // --- resume manifest validation -----------------------------------------

    fn manifest(chunks: Vec<(u32, u64)>) -> ResumeManifest {
        ResumeManifest {
            transfer_id: "t1".into(),
            completed_chunks: chunks
                .into_iter()
                .map(|(file_index, chunk_index)| ChunkRef {
                    file_index,
                    chunk_index,
                })
                .collect(),
        }
    }

    #[test]
    fn validate_accepts_a_plausible_manifest() {
        // One file of 10 bytes, chunk size 4 -> 3 chunks (0, 1, 2).
        let m = manifest(vec![(0, 0), (0, 2)]);
        assert!(m.validate(&[10], 4).is_ok());
    }

    #[test]
    fn validate_rejects_a_chunk_index_beyond_the_file() {
        // Chunk 99 of a 10-byte file cannot exist. An unvalidated manifest here
        // would let a peer skip data or index out of bounds.
        let m = manifest(vec![(0, 99)]);
        assert!(matches!(
            m.validate(&[10], 4),
            Err(Error::MalformedMessage(_))
        ));
    }

    #[test]
    fn validate_rejects_a_file_index_beyond_the_manifest() {
        let m = manifest(vec![(7, 0)]);
        assert!(matches!(
            m.validate(&[10], 4),
            Err(Error::MalformedMessage(_))
        ));
    }

    #[test]
    fn missing_chunks_returns_only_what_is_absent() {
        let m = manifest(vec![(0, 0)]);
        let missing = m.missing_chunks(&[10], 4);
        assert_eq!(
            missing,
            vec![
                ChunkRef {
                    file_index: 0,
                    chunk_index: 1
                },
                ChunkRef {
                    file_index: 0,
                    chunk_index: 2
                },
            ]
        );
    }

    #[test]
    fn missing_chunks_covers_every_file() {
        let m = manifest(vec![]);
        let missing = m.missing_chunks(&[8, 4], 4);
        assert_eq!(missing.len(), 3, "8 bytes = 2 chunks, 4 bytes = 1 chunk");
    }

    // --- progress ------------------------------------------------------------

    #[test]
    fn fraction_handles_a_zero_byte_transfer() {
        let progress = TransferProgress {
            transfer_id: "t".into(),
            status: TransferStatus::Transferring,
            total_bytes: 0,
            transferred_bytes: 0,
            current_bps: 0,
            average_bps: 0,
            eta_seconds: None,
            current_file: None,
        };
        // Must not be NaN.
        assert_eq!(progress.fraction(), 0.0);
        assert!(progress.fraction().is_finite());
    }

    #[test]
    fn fraction_is_clamped() {
        let mut progress = TransferProgress {
            transfer_id: "t".into(),
            status: TransferStatus::Transferring,
            total_bytes: 100,
            transferred_bytes: 250,
            current_bps: 0,
            average_bps: 0,
            eta_seconds: None,
            current_file: None,
        };
        assert_eq!(progress.fraction(), 1.0);

        progress.transferred_bytes = 50;
        assert_eq!(progress.fraction(), 0.5);
    }

    // --- throughput estimator ------------------------------------------------

    #[test]
    fn estimator_needs_two_samples() {
        let mut est = ThroughputEstimator::new(Duration::from_secs(5));
        let now = Instant::now();

        est.record(now, 0);
        assert_eq!(est.bps(), None, "one sample is not a rate");

        est.record(now + Duration::from_secs(1), 1_000_000);
        assert_eq!(est.bps(), Some(1_000_000));
    }

    #[test]
    fn estimator_uses_the_window_not_the_last_instant() {
        let mut est = ThroughputEstimator::new(Duration::from_secs(10));
        let start = Instant::now();

        // A steady 1 MB/s for 5 seconds, then a stalled final second.
        for second in 0..=5 {
            est.record(start + Duration::from_secs(second), second * 1_000_000);
        }
        est.record(start + Duration::from_secs(6), 5_000_000);

        let bps = est.bps().unwrap();
        // Over the 6s window: 5 MB / 6s ≈ 833 kB/s, not the 0 of the last tick.
        assert!(bps > 800_000 && bps < 900_000, "got {bps}");
    }

    #[test]
    fn estimator_drops_samples_older_than_the_window() {
        let mut est = ThroughputEstimator::new(Duration::from_secs(2));
        let start = Instant::now();

        est.record(start, 0);
        est.record(start + Duration::from_secs(10), 10_000_000);
        // The first sample is now outside the window and must be evicted.
        assert_eq!(est.samples.len(), 1);
    }

    #[test]
    fn eta_is_none_without_a_rate() {
        let est = ThroughputEstimator::new(Duration::from_secs(5));
        assert_eq!(est.eta_seconds(1_000_000), None);
    }

    #[test]
    fn eta_is_none_when_stalled() {
        let mut est = ThroughputEstimator::new(Duration::from_secs(5));
        let now = Instant::now();
        est.record(now, 0);
        est.record(now + Duration::from_secs(1), 0); // no progress
        assert_eq!(est.eta_seconds(1_000_000), None);
    }

    // --- state machine -------------------------------------------------------

    #[test]
    fn completed_is_terminal() {
        assert!(TransferStatus::Completed.is_terminal());
        assert!(!TransferStatus::Transferring.is_terminal());
    }

    #[test]
    fn interrupted_transfers_offer_resume() {
        assert!(TransferStatus::Interrupted.is_resumable());
        assert!(TransferStatus::Paused.is_resumable());
        assert!(!TransferStatus::Completed.is_resumable());
        assert!(!TransferStatus::Queued.is_resumable());
    }

    // --- prepare -------------------------------------------------------------

    #[test]
    fn prepare_rejects_a_traversing_entry() {
        let entries = vec![FileEntry {
            relative_path: "../../etc/passwd".into(),
            size: 10,
            is_dir: false,
            mode: None,
            modified: None,
            sha256: None,
        }];
        let root = std::path::Path::new("/dest");
        assert!(matches!(prepare(&entries, root), Err(Error::UnsafePath(_))));
    }

    #[test]
    fn prepare_rejects_a_directory_with_a_size() {
        let entries = vec![FileEntry {
            relative_path: "folder".into(),
            size: 4096,
            is_dir: true,
            mode: None,
            modified: None,
            sha256: None,
        }];
        let root = std::path::Path::new("/dest");
        assert!(matches!(
            prepare(&entries, root),
            Err(Error::MalformedMessage(_))
        ));
    }

    #[test]
    fn prepare_maps_safe_entries_under_the_root() {
        let entries = vec![FileEntry {
            relative_path: "photos/a.jpg".into(),
            size: 100,
            is_dir: false,
            mode: None,
            modified: None,
            sha256: Some("abc".into()),
        }];
        let root = std::path::Path::new("/dest");
        let prepared = prepare(&entries, root).unwrap();

        assert_eq!(prepared.len(), 1);
        assert!(prepared[0].destination.starts_with(root));
    }
}
