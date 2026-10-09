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

    /// Whether the user may pause from here: anything still moving. `Paused`,
    /// `Interrupted` and `Failed` are already stopped, and the terminal states
    /// are over. Without this guard a `Completed` transfer could be paused,
    /// which makes it resumable, which flips it back to `Transferring`.
    pub fn can_pause(self) -> bool {
        !self.is_terminal() && !self.is_resumable()
    }

    /// Whether the user may cancel from here. A `Failed` transfer stays
    /// resumable, so cancelling it is the explicit way to abandon it; only a
    /// transfer that already finished or was already cancelled is refused.
    pub fn can_cancel(self) -> bool {
        !matches!(self, TransferStatus::Completed | TransferStatus::Cancelled)
    }
}

/// Which side of the transfer this machine is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TransferDirection {
    Send,
    Receive,
}

/// A transfer's progress, as the UI needs it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferProgress {
    pub transfer_id: String,
    pub direction: TransferDirection,
    /// Display name of the other computer. Untrusted text from the peer,
    /// length-limited and stripped of control characters before it gets here.
    pub peer_name: String,
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
    /// Why a `Failed` or `Interrupted` transfer stopped, in the structured form
    /// the UI renders (SRS §79: `transfer_failed` carries the error).
    pub error: Option<crate::error::ErrorPayload>,
}

impl TransferProgress {
    /// A transfer that has just been created: nothing moved yet.
    pub fn new(
        transfer_id: String,
        direction: TransferDirection,
        peer_name: String,
        total_bytes: u64,
    ) -> Self {
        Self {
            transfer_id,
            direction,
            peer_name,
            status: TransferStatus::Queued,
            total_bytes,
            transferred_bytes: 0,
            current_bps: 0,
            average_bps: 0,
            eta_seconds: None,
            current_file: None,
            error: None,
        }
    }

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
///
/// A zero `chunk_size` is an error, not a panic: it can be derived from the
/// peer's advertised capabilities, and a remote-triggerable panic aborts the
/// whole process in release builds (`panic = "abort"`).
pub fn chunk_plan(file_size: u64, chunk_size: u32) -> Result<Vec<(u64, u32)>> {
    if chunk_size == 0 {
        return Err(Error::MalformedMessage(
            "chunk size must be non-zero".into(),
        ));
    }

    let chunk_size = chunk_size as u64;
    let mut plan = Vec::new();
    let mut offset = 0u64;

    while offset < file_size {
        let len = chunk_size.min(file_size - offset);
        plan.push((offset, len as u32));
        offset += len;
    }

    Ok(plan)
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

/// Prepares a transfer: validates the peer's manifest of files and resolves
/// every destination path.
///
/// Every path in `entries` is untrusted. This is the choke point where they are
/// proven safe (SRS §91) — the write path later assumes the work is done.
///
/// Also enforced here: the entry-count bound, and that no two entries resolve
/// to the same destination — including case-insensitive collisions (`A.txt` and
/// `a.txt` on Windows and macOS) and a file sharing its name with a directory
/// that holds other entries.
///
/// # What this does not check
///
/// Free disk space. Nothing can know it up front without a platform call, so a
/// full disk is caught when a write fails and reported as `Error::DiskFull`
/// (see `filesystem::map_write_error`). Use [`check_totals`] to confirm the
/// declared byte total and file count against the entries themselves.
pub fn prepare(
    entries: &[FileEntry],
    destination_root: &std::path::Path,
) -> Result<Vec<PreparedItem>> {
    if entries.len() > filesystem::MAX_MANIFEST_ENTRIES {
        return Err(Error::MalformedMessage(format!(
            "manifest has {} entries; the limit is {}",
            entries.len(),
            filesystem::MAX_MANIFEST_ENTRIES
        )));
    }

    let mut prepared = Vec::with_capacity(entries.len());
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut file_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut keys: Vec<(String, bool)> = Vec::with_capacity(entries.len());

    for entry in entries {
        // Reject before joining, so a traversal attempt is reported as such
        // rather than surfacing as an odd I/O error.
        let target = filesystem::safe_join(destination_root, &entry.relative_path)?;

        // A directory has no bytes of its own; a non-zero size is a malformed
        // entry, not something to round or ignore.
        if entry.is_dir && entry.size != 0 {
            return Err(Error::MalformedMessage(format!(
                "directory {} declared a non-zero size",
                entry.relative_path
            )));
        }

        // Compare on the resolved path, lowercased: `./a`, `a` and `A` are one
        // destination on a case-insensitive filesystem.
        let key = target
            .strip_prefix(destination_root)
            .unwrap_or(&target)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect::<Vec<_>>()
            .join("/");
        if !seen.insert(key.clone()) {
            return Err(Error::MalformedMessage(format!(
                "two entries resolve to the same destination: {}",
                entry.relative_path
            )));
        }
        if !entry.is_dir {
            file_keys.insert(key.clone());
        }
        keys.push((key, entry.is_dir));

        prepared.push(PreparedItem {
            relative_path: entry.relative_path.clone(),
            destination: target,
            size: entry.size,
            is_dir: entry.is_dir,
            declared_sha256: entry.sha256.clone(),
        });
    }

    // A file cannot also be a folder: `a` as a file and `a/b` as another entry
    // would have the receiver write through a file.
    for (key, _) in &keys {
        let mut prefix_end = 0;
        while let Some(next) = key[prefix_end..].find('/') {
            prefix_end += next;
            if file_keys.contains(&key[..prefix_end]) {
                return Err(Error::MalformedMessage(format!(
                    "an entry is placed inside a path that is also a file: {key}"
                )));
            }
            prefix_end += 1;
        }
    }

    Ok(prepared)
}

/// Confirms the declared totals against the entries themselves.
///
/// The peer states a byte total and a file count separately from the entries;
/// believing either without checking lets it lie to the progress bar, or hide a
/// size the destination cannot hold (SRS §92). Returns the verified byte total.
pub fn check_totals(
    entries: &[FileEntry],
    declared_bytes: u64,
    declared_files: u64,
) -> Result<u64> {
    let mut total: u64 = 0;
    let mut files: u64 = 0;
    for entry in entries.iter().filter(|e| !e.is_dir) {
        total = total
            .checked_add(entry.size)
            .ok_or_else(|| Error::MalformedMessage("manifest byte total overflows".into()))?;
        files += 1;
    }
    if total != declared_bytes {
        return Err(Error::MalformedMessage(format!(
            "declared {declared_bytes} bytes but the entries add up to {total}"
        )));
    }
    if files != declared_files {
        return Err(Error::MalformedMessage(format!(
            "declared {declared_files} files but the manifest lists {files}"
        )));
    }
    Ok(total)
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
        let plan = chunk_plan(10, 4).unwrap();
        assert_eq!(plan, vec![(0, 4), (4, 4), (8, 2)]);

        let total: u64 = plan.iter().map(|(_, len)| *len as u64).sum();
        assert_eq!(total, 10);
    }

    #[test]
    fn chunk_plan_of_an_exact_multiple_has_no_short_tail() {
        let plan = chunk_plan(8, 4).unwrap();
        assert_eq!(plan, vec![(0, 4), (4, 4)]);
    }

    #[test]
    fn chunk_plan_of_an_empty_file_is_empty() {
        assert!(chunk_plan(0, 4).unwrap().is_empty());
        assert_eq!(chunk_count(0, 4), 0);
    }

    #[test]
    fn chunk_plan_rejects_a_zero_chunk_size_instead_of_panicking() {
        // A peer can advertise `max_chunk_size: 0`; that must surface as an
        // error, because a panic here aborts the process in release builds.
        assert!(matches!(
            chunk_plan(1000, 0),
            Err(Error::MalformedMessage(_))
        ));
        assert!(matches!(chunk_plan(0, 0), Err(Error::MalformedMessage(_))));
    }

    #[test]
    fn chunk_plan_chunks_are_contiguous() {
        let plan = chunk_plan(1000, 64).unwrap();
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
                chunk_plan(size, 4096).unwrap().len(),
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
        let mut progress =
            TransferProgress::new("t".into(), TransferDirection::Send, "peer".into(), 0);
        progress.status = TransferStatus::Transferring;
        // Must not be NaN.
        assert_eq!(progress.fraction(), 0.0);
        assert!(progress.fraction().is_finite());
    }

    #[test]
    fn fraction_is_clamped() {
        let mut progress =
            TransferProgress::new("t".into(), TransferDirection::Receive, "peer".into(), 100);
        progress.status = TransferStatus::Transferring;
        progress.transferred_bytes = 250;
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

    #[test]
    fn only_a_moving_transfer_can_be_paused() {
        use TransferStatus::*;
        for status in [
            Queued,
            Preparing,
            Connecting,
            Authenticating,
            Transferring,
            Verifying,
        ] {
            assert!(status.can_pause(), "{status:?} should be pausable");
        }
        // Already stopped or already over. Pausing a `Completed` transfer used
        // to make it resumable and then flip it back to `Transferring`.
        for status in [Paused, Interrupted, Failed, Completed, Cancelled] {
            assert!(!status.can_pause(), "{status:?} should not be pausable");
        }
    }

    #[test]
    fn a_finished_or_cancelled_transfer_cannot_be_cancelled() {
        use TransferStatus::*;
        assert!(!Completed.can_cancel());
        assert!(!Cancelled.can_cancel());
        for status in [
            Queued,
            Preparing,
            Connecting,
            Authenticating,
            Transferring,
            Verifying,
            Paused,
            Interrupted,
        ] {
            assert!(status.can_cancel(), "{status:?} should be cancellable");
        }
    }

    #[test]
    fn a_failed_transfer_can_still_be_abandoned() {
        // `Failed` is resumable, so cancel is the only way to give up on it.
        assert!(TransferStatus::Failed.is_resumable());
        assert!(TransferStatus::Failed.can_cancel());
    }

    #[test]
    fn no_state_is_both_pausable_and_resumable() {
        // If one were, pause and resume could ping-pong a transfer forever.
        use TransferStatus::*;
        for status in [
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
        ] {
            assert!(
                !(status.can_pause() && status.is_resumable()),
                "{status:?} is both pausable and resumable"
            );
        }
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

    fn entry(path: &str, size: u64, is_dir: bool) -> FileEntry {
        FileEntry {
            relative_path: path.into(),
            size,
            is_dir,
            mode: None,
            modified: None,
            sha256: None,
        }
    }

    fn malformed<T: std::fmt::Debug>(r: Result<T>) -> bool {
        matches!(r, Err(Error::MalformedMessage(_)))
    }

    #[test]
    fn prepare_rejects_two_entries_for_the_same_destination() {
        let root = std::path::Path::new("/dest");
        let dup = vec![entry("a.txt", 1, false), entry("./a.txt", 2, false)];
        assert!(malformed(prepare(&dup, root)));
    }

    #[test]
    fn prepare_rejects_names_that_differ_only_by_case() {
        // One file on Windows and macOS, so the second would overwrite the first.
        let root = std::path::Path::new("/dest");
        let clash = vec![entry("Report.txt", 1, false), entry("report.TXT", 2, false)];
        assert!(malformed(prepare(&clash, root)));
    }

    #[test]
    fn prepare_rejects_an_entry_inside_a_file() {
        let root = std::path::Path::new("/dest");
        let through_a_file = vec![entry("a", 1, false), entry("a/b.txt", 1, false)];
        assert!(malformed(prepare(&through_a_file, root)));
    }

    #[test]
    fn prepare_accepts_a_directory_followed_by_its_contents() {
        let root = std::path::Path::new("/dest");
        let ok = vec![
            entry("photos", 0, true),
            entry("photos/2024", 0, true),
            entry("photos/2024/a.jpg", 5, false),
            entry("photos/b.jpg", 7, false),
        ];
        assert_eq!(prepare(&ok, root).unwrap().len(), 4);
    }

    #[test]
    fn prepare_rejects_a_manifest_over_the_entry_limit() {
        let root = std::path::Path::new("/dest");
        let too_many: Vec<FileEntry> = (0..=filesystem::MAX_MANIFEST_ENTRIES)
            .map(|i| entry(&format!("f{i}"), 0, false))
            .collect();
        assert!(malformed(prepare(&too_many, root)));
    }

    #[test]
    fn check_totals_accepts_an_honest_manifest() {
        let entries = vec![
            entry("d", 0, true),
            entry("d/a", 10, false),
            entry("d/b", 5, false),
        ];
        assert_eq!(check_totals(&entries, 15, 2).unwrap(), 15);
    }

    #[test]
    fn check_totals_rejects_a_lie_about_bytes_or_files() {
        let entries = vec![entry("a", 10, false), entry("b", 5, false)];
        // Claiming fewer bytes than the entries hold would hide the real size
        // from the destination-capacity check and the progress bar.
        assert!(malformed(check_totals(&entries, 1, 2)));
        assert!(malformed(check_totals(&entries, 15, 99)));
    }

    #[test]
    fn check_totals_survives_sizes_that_overflow() {
        let entries = vec![entry("a", u64::MAX, false), entry("b", 1, false)];
        assert!(malformed(check_totals(&entries, 0, 2)));
    }
}
