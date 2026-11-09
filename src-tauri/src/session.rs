//! One connection, one transfer: the sender and receiver state machines.
//!
//! Implements specs F04 (session and wire protocol), F05 (file and folder
//! transfer), F06 (chunking, resume, integrity) and the receiving half of F07
//! (filesystem safety). Written against any `AsyncRead + AsyncWrite`, so the
//! loopback tests and a real TCP socket run the same code (D-F05-7).
//!
//! # The conversation (D-F04-2)
//!
//! ```text
//! sender                                   receiver
//!   Hello ───────────────────────────────▶   validate, negotiate
//!   ◀─────────────────────────────── HelloAck   (or Error versionMismatch)
//!   TransferRequest ─────────────────────▶   validate, ask the user
//!   ◀──────────────── Accept{completed} / Reject
//!   ChunkHeader, raw bytes, ChunkFooter ──▶   check length + SHA-256, write
//!   … one per missing chunk …
//!   TransferComplete{digests} ───────────▶   re-hash part files from disk
//!   ◀──────────────── Verified / VerificationFailed
//! ```
//!
//! Either side may send `Error{code}` at any point; `cancelled` and `paused`
//! are the user's stop buttons.
//!
//! # Security status
//!
//! There is no encryption or authentication yet (F08). By owner decision
//! D-F08-7 the plaintext path runs only in development builds: in a release
//! build both entry points refuse with `SecureSessionRequired` before touching
//! the stream. Until F08, the receiver's per-transfer approval is the only
//! gate, and SHA-256 proves integrity, not who sent the data (F06 finding 4).

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::error::{Error, Result};
use crate::filesystem::{self, ConflictResolution, Finalized, LocalItem};
use crate::protocol::{
    self, read_message, write_message, Capabilities, ChunkRef, FileDigest, FileEntry, Message,
    ProtocolVersion, DEFAULT_CHUNK_SIZE, PROTOCOL_VERSION,
};
use crate::transfer::{
    self, chunk_count, ChunkBuffer, ResumeManifest, ThroughputEstimator, TransferDirection,
    TransferProgress, TransferStatus, PIPELINE_DEPTH,
};
use crate::transport::{TcpTransport, Transport, TransportKind, TransportStream};

/// Whether this build may transfer without encryption (D-F08-7). True only for
/// development builds; a release build refuses at both entry points.
pub const PLAINTEXT_ALLOWED: bool = cfg!(debug_assertions);

/// Most receive sessions kept for resume at once (SRS §93).
const MAX_RESUMABLE_SESSIONS: usize = 64;

/// How often progress is reported while bytes are moving (D-F05-4).
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Who this machine is, as told to the peer.
#[derive(Debug, Clone)]
pub struct LocalIdentity {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
}

/// Tunables for one session. `Default` is production (D-F04-5).
#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub max_chunk_size: u32,
    pub protocol: ProtocolVersion,
    pub handshake_timeout: Duration,
    /// How long the receiving user has to answer before the request is
    /// rejected.
    pub decision_timeout: Duration,
    /// How long data may stop flowing before the connection counts as lost.
    pub idle_timeout: Duration,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            max_chunk_size: DEFAULT_CHUNK_SIZE,
            protocol: PROTOCOL_VERSION,
            handshake_timeout: Duration::from_secs(10),
            decision_timeout: Duration::from_secs(120),
            idle_timeout: Duration::from_secs(30),
        }
    }
}

impl SessionOptions {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            protocol: self.protocol,
            max_chunk_size: self.max_chunk_size,
            ..Capabilities::default()
        }
    }
}

/// Receives every progress update the engine reports.
pub type ProgressSink = Arc<dyn Fn(TransferProgress) + Send + Sync>;

/// Why a transfer was stopped by its user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Pause,
    Cancel,
}

/// The stop button for one running transfer. Cloning shares it.
#[derive(Clone, Default)]
pub struct Control {
    token: CancellationToken,
    reason: Arc<Mutex<Option<StopReason>>>,
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control")
            .field("reason", &self.reason())
            .finish()
    }
}

impl Control {
    pub fn new() -> Self {
        Self::default()
    }

    /// Stops the transfer. The first reason given wins.
    pub fn stop(&self, reason: StopReason) {
        {
            let mut r = match self.reason.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            r.get_or_insert(reason);
        }
        self.token.cancel();
    }

    pub fn reason(&self) -> Option<StopReason> {
        match self.reason.lock() {
            Ok(g) => *g,
            Err(p) => *p.into_inner(),
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.token.is_cancelled()
    }

    async fn stopped(&self) {
        self.token.cancelled().await;
    }
}

/// What to send: the transfer id and the manifest with local sources.
#[derive(Debug, Clone)]
pub struct SendJob {
    pub transfer_id: String,
    pub items: Vec<LocalItem>,
}

/// How a completed send went. Throughput is measured, never derived (§101).
#[derive(Debug, Clone)]
pub struct SendReport {
    pub bytes: u64,
    pub chunks_sent: u64,
    pub elapsed: Duration,
    pub average_bps: u64,
}

/// What the receiving user is asked to approve (SRS §48, D-F05-1). Every text
/// field came from the peer and has been sanitised.
#[derive(Debug, Clone)]
pub struct IncomingRequest {
    pub transfer_id: String,
    pub peer_device_id: String,
    pub peer_name: String,
    pub peer_platform: String,
    pub file_count: u64,
    pub total_bytes: u64,
    /// Up to ten top-level names, for the dialog.
    pub top_level: Vec<String>,
    pub destination: PathBuf,
    pub resuming: bool,
}

/// The receiving user's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Accept,
    Reject(String),
}

/// The receiving side's UI: asks the user and hands over the stop button.
#[async_trait::async_trait]
pub trait IncomingHandler: Send + Sync {
    /// Asks the user. Bounded by `SessionOptions::decision_timeout`.
    async fn decide(&self, request: IncomingRequest, control: Control) -> Decision;

    /// An approved transfer is resuming without a new question (D-F06-4).
    fn resumed(&self, _request: IncomingRequest, _control: Control) {}
}

/// Where and how received files land.
#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    pub destination: PathBuf,
    pub conflict: ConflictResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveOutcome {
    Completed,
    Rejected,
    /// The sender paused; part files and the session are kept for resume.
    Paused,
}

#[derive(Debug, Clone)]
pub struct ReceiveReport {
    pub transfer_id: String,
    pub outcome: ReceiveOutcome,
    pub bytes: u64,
    /// Final paths of the files written.
    pub files: Vec<PathBuf>,
    /// Files skipped because of the conflict policy.
    pub skipped: usize,
}

/// One receive session kept for resume within this process (D-F06-4).
struct ReceiveSession {
    peer_device_id: String,
    fingerprint: String,
    completed: Mutex<HashSet<(u32, u64)>>,
}

impl ReceiveSession {
    fn completed(&self) -> HashSet<(u32, u64)> {
        match self.completed.lock() {
            Ok(g) => g.clone(),
            Err(p) => p.into_inner().clone(),
        }
    }

    fn mark(&self, chunk: (u32, u64)) {
        match self.completed.lock() {
            Ok(mut g) => {
                g.insert(chunk);
            }
            Err(p) => {
                p.into_inner().insert(chunk);
            }
        }
    }
}

/// The receive sessions this process can resume.
#[derive(Default)]
pub struct ReceiverRegistry {
    sessions: Mutex<HashMap<String, Arc<ReceiveSession>>>,
}

impl ReceiverRegistry {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Arc<ReceiveSession>>> {
        match self.sessions.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// How many chunks of a transfer have landed, if it is resumable.
    pub fn completed_chunks(&self, transfer_id: &str) -> Option<usize> {
        self.lock().get(transfer_id).map(|s| s.completed().len())
    }

    fn get(&self, transfer_id: &str) -> Option<Arc<ReceiveSession>> {
        self.lock().get(transfer_id).cloned()
    }

    fn insert(&self, transfer_id: String, session: Arc<ReceiveSession>) {
        let mut map = self.lock();
        if map.len() >= MAX_RESUMABLE_SESSIONS && !map.contains_key(&transfer_id) {
            if let Some(oldest) = map.keys().next().cloned() {
                map.remove(&oldest);
            }
        }
        map.insert(transfer_id, session);
    }

    fn remove(&self, transfer_id: &str) {
        self.lock().remove(transfer_id);
    }
}

/// Lowercase hex, as digests travel in `FileDigest` and `VerificationFailed`.
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Identifies a manifest, so a resume only continues the transfer the user
/// approved (D-F06-4).
fn fingerprint(entries: &[FileEntry], chunk_size: u32) -> String {
    let mut h = Sha256::new();
    h.update(chunk_size.to_be_bytes());
    for e in entries {
        h.update(e.relative_path.as_bytes());
        h.update([0, u8::from(e.is_dir)]);
        h.update(e.size.to_be_bytes());
    }
    hex(&h.finalize())
}

fn chunk_len(file_size: u64, chunk_size: u32, index: u64) -> u32 {
    let start = index * u64::from(chunk_size);
    file_size.saturating_sub(start).min(u64::from(chunk_size)) as u32
}

/// Runs `fut` with a deadline; running out counts as a lost connection.
async fn timed<T>(limit: Duration, fut: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(limit, fut)
        .await
        .map_err(|_| Error::ConnectionLost {
            transferred: 0,
            resumable: true,
        })?
}

/// Turns an I/O failure into "connection lost" with the bytes so far; leaves
/// every other error alone.
fn lost(e: Error, transferred: u64) -> Error {
    match e {
        Error::Io(_) | Error::ConnectionLost { .. } => Error::ConnectionLost {
            transferred,
            resumable: true,
        },
        other => other,
    }
}

/// Stops a background task when dropped, so a split stream's read half never
/// outlives the session and keeps the connection half-open.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Reports progress at most every 250 ms while bytes move, and on every status
/// change (D-F05-4).
struct Reporter {
    sink: ProgressSink,
    progress: TransferProgress,
    estimator: ThroughputEstimator,
    started: Instant,
    last_data_emit: Option<Instant>,
}

impl Reporter {
    fn new(sink: ProgressSink, progress: TransferProgress) -> Self {
        Self {
            sink,
            progress,
            estimator: ThroughputEstimator::new(Duration::from_secs(5)),
            started: Instant::now(),
            last_data_emit: None,
        }
    }

    fn status(&mut self, status: TransferStatus) {
        self.progress.status = status;
        self.emit();
    }

    /// Sets the starting point (bytes already held on a resume) without
    /// counting as a data update, so the first chunk is always reported.
    fn baseline(&mut self, transferred: u64) {
        self.progress.transferred_bytes = transferred;
        self.estimator.record(Instant::now(), transferred);
    }

    fn bytes(&mut self, transferred: u64, current_file: Option<&str>) {
        let now = Instant::now();
        self.progress.transferred_bytes = transferred;
        self.progress.current_file = current_file.map(str::to_owned);
        self.estimator.record(now, transferred);
        let due = match self.last_data_emit {
            None => true,
            Some(t) => now.duration_since(t) >= PROGRESS_INTERVAL,
        };
        if due {
            self.last_data_emit = Some(now);
            self.emit();
        }
    }

    fn emit(&mut self) {
        let elapsed = self.started.elapsed().as_secs_f64();
        self.progress.current_bps = self.estimator.bps().unwrap_or(0);
        if elapsed > 0.0 {
            self.progress.average_bps = (self.progress.transferred_bytes as f64 / elapsed) as u64;
        }
        let remaining = self
            .progress
            .total_bytes
            .saturating_sub(self.progress.transferred_bytes);
        self.progress.eta_seconds = self.estimator.eta_seconds(remaining);
        (self.sink)(self.progress.clone());
    }
}

/// Connects to the first address that answers, in order (F02 fallback,
/// D-F02-4). Returns the stream and the address used.
pub async fn connect_first(addrs: &[SocketAddr]) -> Result<(Box<dyn TransportStream>, SocketAddr)> {
    let transport = TcpTransport::new(TransportKind::Ethernet);
    for addr in addrs {
        match transport.connect(*addr).await {
            Ok(stream) => return Ok((stream, *addr)),
            Err(e) => debug!(%addr, error = %e, "endpoint refused; trying the next"),
        }
    }
    Err(Error::PeerNotFound)
}

/// Best effort: tell the peer why we are stopping, then close our side.
async fn send_stop<W: AsyncWrite + Unpin>(writer: &mut W, transfer_id: &str, code: &str) {
    let _ = write_message(
        writer,
        &Message::Error {
            transfer_id: Some(transfer_id.to_string()),
            code: code.to_string(),
            detail: String::new(),
        },
    )
    .await;
    let _ = writer.shutdown().await;
}

fn stop_code(reason: Option<StopReason>) -> &'static str {
    match reason {
        Some(StopReason::Pause) => "paused",
        _ => "cancelled",
    }
}

/// What the sender's disk reader hands the network writer.
enum Outgoing {
    Chunk {
        file_index: u32,
        chunk_index: u64,
        data: Vec<u8>,
        digest: [u8; 32],
    },
    FileDone {
        file_index: u32,
        digest: String,
    },
}

/// Reads every file in order on a blocking thread, hashing each chunk and each
/// whole file, and queues the chunks the receiver is missing. The bounded
/// channel is the backpressure (SRS §29): when the network is slower than the
/// disk, `blocking_send` waits.
fn spawn_reader(
    items: Vec<LocalItem>,
    chunk_size: u32,
    have: HashSet<(u32, u64)>,
    tx: mpsc::Sender<Outgoing>,
) -> tokio::task::JoinHandle<Result<()>> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        for (index, item) in items.iter().enumerate() {
            if item.entry.is_dir {
                continue;
            }
            let file_index = index as u32;
            let mut file = std::fs::File::open(&item.source)?;
            // §89 "source file changes": refuse rather than send a different file.
            if file.metadata()?.len() != item.entry.size {
                return Err(Error::Io(std::io::Error::other(format!(
                    "{} changed size since it was chosen",
                    item.source.display()
                ))));
            }
            let mut whole = Sha256::new();
            for chunk_index in 0..chunk_count(item.entry.size, chunk_size) {
                let len = chunk_len(item.entry.size, chunk_size, chunk_index) as usize;
                let mut data = vec![0u8; len];
                file.read_exact(&mut data)?;
                whole.update(&data);
                if have.contains(&(file_index, chunk_index)) {
                    continue;
                }
                let digest: [u8; 32] = Sha256::digest(&data).into();
                let chunk = Outgoing::Chunk {
                    file_index,
                    chunk_index,
                    data,
                    digest,
                };
                if tx.blocking_send(chunk).is_err() {
                    return Ok(()); // the session ended; nothing more to do
                }
            }
            let done = Outgoing::FileDone {
                file_index,
                digest: hex(&whole.finalize()),
            };
            if tx.blocking_send(done).is_err() {
                return Ok(());
            }
        }
        Ok(())
    })
}

/// Maps an `Error` message from the peer to what the local user should see.
fn peer_error(code: &str, detail: &str) -> Error {
    match code {
        "cancelled" | "paused" => Error::Cancelled,
        _ => Error::MalformedMessage(format!(
            "the other computer stopped the transfer ({code}) {detail}"
        )),
    }
}

/// Sends a transfer (the connecting side is the sender, D-F04 / CR-05).
pub async fn run_sender<S>(
    stream: S,
    me: &LocalIdentity,
    opts: &SessionOptions,
    job: SendJob,
    control: Control,
    progress: ProgressSink,
) -> Result<SendReport>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    if !PLAINTEXT_ALLOWED {
        return Err(Error::SecureSessionRequired);
    }
    let started = Instant::now();
    let (mut rd, mut wr) = tokio::io::split(stream);
    let caps = opts.capabilities();

    // --- handshake ------------------------------------------------------------
    let nonce: [u8; 32] = rand::random();
    timed(
        opts.handshake_timeout,
        write_message(
            &mut wr,
            &Message::Hello {
                device_id: me.device_id.clone(),
                display_name: me.display_name.clone(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                platform: me.platform.clone(),
                capabilities: caps.clone(),
                nonce,
            },
        ),
    )
    .await?;

    let (peer_name, agreed) = match timed(opts.handshake_timeout, read_message(&mut rd)).await? {
        Message::HelloAck {
            device_id,
            display_name,
            capabilities,
            peer_nonce,
            ..
        } => {
            if peer_nonce != nonce {
                return Err(Error::MalformedMessage(
                    "handshake reply belongs to another connection".into(),
                ));
            }
            if !protocol::is_valid_device_id(&device_id) {
                return Err(Error::MalformedMessage(
                    "peer device id is malformed".into(),
                ));
            }
            let name =
                protocol::sanitize_remote_text(&display_name, protocol::MAX_DISPLAY_NAME_CHARS);
            (name, caps.intersect(&capabilities)?)
        }
        Message::Error { code, detail, .. } if code == "versionMismatch" => {
            return Err(Error::VersionMismatch {
                local: opts.protocol.to_string(),
                remote: protocol::sanitize_remote_text(&detail, 16),
            })
        }
        Message::Error { code, detail, .. } => return Err(peer_error(&code, &detail)),
        other => {
            return Err(Error::MalformedMessage(format!(
                "expected HelloAck, got {}",
                message_name(&other)
            )))
        }
    };
    let chunk_size = agreed.max_chunk_size;

    // --- request ----------------------------------------------------------------
    let entries: Vec<FileEntry> = job.items.iter().map(|i| i.entry.clone()).collect();
    let sizes: Vec<u64> = entries
        .iter()
        .map(|e| if e.is_dir { 0 } else { e.size })
        .collect();
    let total: u64 = sizes.iter().sum();
    let file_count = entries.iter().filter(|e| !e.is_dir).count() as u64;
    let mut reporter = Reporter::new(
        progress,
        TransferProgress::new(
            job.transfer_id.clone(),
            TransferDirection::Send,
            peer_name,
            total,
        ),
    );
    reporter.status(TransferStatus::Connecting);

    timed(
        opts.handshake_timeout,
        write_message(
            &mut wr,
            &Message::TransferRequest {
                transfer_id: job.transfer_id.clone(),
                total_bytes: total,
                file_count,
                entries: entries.clone(),
            },
        ),
    )
    .await?;

    let answer = tokio::select! {
        biased;
        _ = control.stopped() => {
            send_stop(&mut wr, &job.transfer_id, stop_code(control.reason())).await;
            return Err(Error::Cancelled);
        }
        m = timed(opts.decision_timeout + opts.handshake_timeout, read_message(&mut rd)) => m?,
    };
    let completed = match answer {
        Message::Accept {
            transfer_id,
            completed_chunks,
        } if transfer_id == job.transfer_id => completed_chunks,
        Message::Reject { .. } => return Err(Error::PeerRejected),
        Message::Error { code, detail, .. } => return Err(peer_error(&code, &detail)),
        other => {
            return Err(Error::MalformedMessage(format!(
                "expected Accept or Reject, got {}",
                message_name(&other)
            )))
        }
    };

    // The receiver's resume claim is remote input (F06).
    let manifest = ResumeManifest {
        transfer_id: job.transfer_id.clone(),
        completed_chunks: completed,
    };
    manifest.validate(&sizes, chunk_size)?;
    let have: HashSet<(u32, u64)> = manifest
        .completed_chunks
        .iter()
        .map(|c| (c.file_index, c.chunk_index))
        .collect();
    let already: u64 = have
        .iter()
        .map(|(f, c)| u64::from(chunk_len(sizes[*f as usize], chunk_size, *c)))
        .sum();

    // --- data -------------------------------------------------------------------
    let (peer_tx, mut peer_rx) = mpsc::channel::<Result<Message>>(4);
    let _peer_reader = AbortOnDrop(tokio::spawn(async move {
        loop {
            let msg = read_message(&mut rd).await;
            let end = msg.is_err();
            if peer_tx.send(msg).await.is_err() || end {
                break;
            }
        }
    }));

    let (chunk_tx, mut chunk_rx) = mpsc::channel::<Outgoing>(PIPELINE_DEPTH);
    let reader = spawn_reader(job.items.clone(), chunk_size, have, chunk_tx);

    reporter.baseline(already);
    reporter.status(TransferStatus::Transferring);
    let mut sent = already;
    let mut chunks_sent = 0u64;
    let mut digests = Vec::new();

    loop {
        tokio::select! {
            biased;
            _ = control.stopped() => {
                drop(chunk_rx);
                send_stop(&mut wr, &job.transfer_id, stop_code(control.reason())).await;
                return Err(Error::Cancelled);
            }
            msg = peer_rx.recv() => {
                return Err(match msg {
                    Some(Ok(Message::Error { code, detail, .. })) => peer_error(&code, &detail),
                    Some(Ok(Message::VerificationFailed { relative_path, expected, actual, .. })) => {
                        Error::VerificationFailed { path: relative_path, expected, actual }
                    }
                    Some(Ok(other)) => Error::MalformedMessage(format!(
                        "unexpected {} during the transfer",
                        message_name(&other)
                    )),
                    Some(Err(_)) | None => Error::ConnectionLost { transferred: sent, resumable: true },
                });
            }
            item = chunk_rx.recv() => match item {
                Some(Outgoing::Chunk { file_index, chunk_index, data, digest }) => {
                    let header = Message::ChunkHeader {
                        transfer_id: job.transfer_id.clone(),
                        file_index,
                        chunk_index,
                        size: data.len() as u32,
                    };
                    let footer = Message::ChunkFooter {
                        transfer_id: job.transfer_id.clone(),
                        file_index,
                        chunk_index,
                        sha256: digest,
                    };
                    let write = timed(opts.idle_timeout, async {
                        write_message(&mut wr, &header).await?;
                        wr.write_all(&data).await?;
                        write_message(&mut wr, &footer).await
                    })
                    .await;
                    if let Err(e) = write {
                        return Err(lost(e, sent));
                    }
                    sent += data.len() as u64;
                    chunks_sent += 1;
                    reporter.bytes(sent, Some(&entries[file_index as usize].relative_path));
                }
                Some(Outgoing::FileDone { file_index, digest }) => {
                    digests.push(FileDigest { file_index, sha256: digest });
                }
                None => break,
            },
        }
    }

    // The reader stopped: either it finished or it failed.
    match reader.await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            send_stop(&mut wr, &job.transfer_id, "sourceChanged").await;
            return Err(e);
        }
        Err(e) => return Err(Error::Other(anyhow::anyhow!("file reader stopped: {e}"))),
    }

    timed(
        opts.idle_timeout,
        write_message(
            &mut wr,
            &Message::TransferComplete {
                transfer_id: job.transfer_id.clone(),
                digests,
            },
        ),
    )
    .await
    .map_err(|e| lost(e, sent))?;
    reporter.status(TransferStatus::Verifying);

    // Verification time grows with file size, so no deadline here; the user
    // can still stop it, and a dropped connection still ends it.
    let verdict = tokio::select! {
        biased;
        _ = control.stopped() => {
            send_stop(&mut wr, &job.transfer_id, stop_code(control.reason())).await;
            return Err(Error::Cancelled);
        }
        m = peer_rx.recv() => m,
    };
    match verdict {
        Some(Ok(Message::Verified { transfer_id })) if transfer_id == job.transfer_id => {}
        Some(Ok(Message::VerificationFailed {
            relative_path,
            expected,
            actual,
            ..
        })) => {
            return Err(Error::VerificationFailed {
                path: relative_path,
                expected,
                actual,
            })
        }
        Some(Ok(Message::Error { code, detail, .. })) => return Err(peer_error(&code, &detail)),
        Some(Ok(other)) => {
            return Err(Error::MalformedMessage(format!(
                "expected Verified, got {}",
                message_name(&other)
            )))
        }
        Some(Err(_)) | None => {
            return Err(Error::ConnectionLost {
                transferred: sent,
                resumable: true,
            })
        }
    }

    let _ = wr.shutdown().await;
    reporter.status(TransferStatus::Completed);
    let elapsed = started.elapsed();
    let moved = sent - already;
    let average_bps = if elapsed.as_secs_f64() > 0.0 {
        (moved as f64 / elapsed.as_secs_f64()) as u64
    } else {
        0
    };
    info!(transfer_id = %job.transfer_id, bytes = total, chunks_sent, average_bps, "transfer sent and verified");
    Ok(SendReport {
        bytes: total,
        chunks_sent,
        elapsed,
        average_bps,
    })
}

fn message_name(m: &Message) -> &'static str {
    match m {
        Message::Hello { .. } => "Hello",
        Message::HelloAck { .. } => "HelloAck",
        Message::Authenticate { .. } => "Authenticate",
        Message::AuthOk { .. } => "AuthOk",
        Message::AuthFailed { .. } => "AuthFailed",
        Message::TransferRequest { .. } => "TransferRequest",
        Message::Accept { .. } => "Accept",
        Message::Reject { .. } => "Reject",
        Message::ResumeQuery { .. } => "ResumeQuery",
        Message::ResumeManifest { .. } => "ResumeManifest",
        Message::ChunkHeader { .. } => "ChunkHeader",
        Message::ChunkFooter { .. } => "ChunkFooter",
        Message::TransferComplete { .. } => "TransferComplete",
        Message::Verified { .. } => "Verified",
        Message::VerificationFailed { .. } => "VerificationFailed",
        Message::Error { .. } => "Error",
    }
}

/// Top-level names for the approval dialog: at most ten, sanitised.
fn top_level_names(entries: &[FileEntry]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for e in entries {
        let first = e
            .relative_path
            .split(['/', '\\'])
            .next()
            .unwrap_or_default();
        let name = protocol::sanitize_remote_text(first, 128);
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
            if names.len() == 10 {
                break;
            }
        }
    }
    names
}

/// The receiver's plan for one file or directory.
struct Target {
    entry: FileEntry,
    destination: PathBuf,
    /// `None` for directories.
    part: Option<PathBuf>,
}

/// Writes verified chunks to their part files, in arrival order, and marks each
/// complete only after its write returned (FR-CHUNK-001, D-F06-1).
async fn write_chunks(
    mut rx: mpsc::Receiver<ChunkBuffer>,
    parts: Arc<Vec<Option<PathBuf>>>,
    sizes: Arc<Vec<u64>>,
    session: Arc<ReceiveSession>,
) -> Result<()> {
    use tokio::io::AsyncSeekExt;
    let mut current: Option<(u32, tokio::fs::File)> = None;
    let mut remaining: u64 = sizes.iter().sum();

    while let Some(chunk) = rx.recv().await {
        let index = chunk.file_index as usize;
        if current.as_ref().map(|(i, _)| *i) != Some(chunk.file_index) {
            let Some(path) = parts.get(index).cloned().flatten() else {
                return Err(Error::MalformedMessage("chunk for a directory".into()));
            };
            let file = tokio::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)
                .await
                .map_err(|e| filesystem::map_write_error(e, remaining))?;
            // Pre-size the part file, so a destination that cannot hold the
            // file fails here rather than at the end (D-F07-7).
            if file.metadata().await?.len() != sizes[index] {
                file.set_len(sizes[index])
                    .await
                    .map_err(|e| filesystem::map_write_error(e, remaining))?;
            }
            current = Some((chunk.file_index, file));
        }
        let Some((_, file)) = current.as_mut() else {
            continue;
        };
        file.seek(std::io::SeekFrom::Start(chunk.offset)).await?;
        file.write_all(&chunk.data)
            .await
            .map_err(|e| filesystem::map_write_error(e, remaining))?;
        remaining = remaining.saturating_sub(chunk.data.len() as u64);
        session.mark((chunk.file_index, chunk.chunk_index));
    }
    if let Some((_, mut file)) = current {
        file.flush().await?;
    }
    Ok(())
}

/// Hashes a file from disk on a blocking thread.
async fn hash_file(path: PathBuf) -> Result<String> {
    tokio::task::spawn_blocking(move || -> Result<String> {
        use std::io::Read;
        let mut file = std::fs::File::open(&path)?;
        let mut h = Sha256::new();
        let mut buf = vec![0u8; 1024 * 1024];
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(hex(&h.finalize()))
    })
    .await
    .map_err(|e| Error::Other(anyhow::anyhow!("hashing stopped: {e}")))?
}

/// Deletes every part file of a transfer. Best effort.
async fn remove_parts(targets: &[Target]) {
    for t in targets {
        if let Some(part) = &t.part {
            let _ = tokio::fs::remove_file(part).await;
        }
    }
}

/// How the receiver's data loop ended.
enum LoopEnd {
    Complete(Vec<FileDigest>),
    LocalStop(Option<StopReason>),
    PeerStop(StopReason),
    Lost,
    Protocol(Error),
    Corrupt {
        path: String,
        expected: String,
        actual: String,
    },
    WriterFailed,
}

/// Receives a transfer (the accepting side, D-F04-2).
#[allow(clippy::too_many_arguments)]
pub async fn run_receiver<S>(
    stream: S,
    me: &LocalIdentity,
    opts: &SessionOptions,
    config: &ReceiverConfig,
    handler: Arc<dyn IncomingHandler>,
    registry: Arc<ReceiverRegistry>,
    progress: ProgressSink,
) -> Result<ReceiveReport>
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    if !PLAINTEXT_ALLOWED {
        return Err(Error::SecureSessionRequired);
    }
    let mut stream = stream;
    let caps = opts.capabilities();

    // --- handshake ------------------------------------------------------------
    let (peer_id, peer_name, peer_platform, agreed) =
        match timed(opts.handshake_timeout, read_message(&mut stream)).await? {
            Message::Hello {
                device_id,
                display_name,
                platform,
                capabilities,
                nonce,
                ..
            } => {
                if !protocol::is_valid_device_id(&device_id) {
                    return Err(Error::MalformedMessage(
                        "peer device id is malformed".into(),
                    ));
                }
                let agreed = match caps.intersect(&capabilities) {
                    Ok(a) => a,
                    Err(e) => {
                        let code = match e {
                            Error::VersionMismatch { .. } => "versionMismatch",
                            _ => "protocolError",
                        };
                        let detail = if code == "versionMismatch" {
                            opts.protocol.to_string()
                        } else {
                            e.to_string()
                        };
                        let _ = write_message(
                            &mut stream,
                            &Message::Error {
                                transfer_id: None,
                                code: code.into(),
                                detail,
                            },
                        )
                        .await;
                        return Err(e);
                    }
                };
                timed(
                    opts.handshake_timeout,
                    write_message(
                        &mut stream,
                        &Message::HelloAck {
                            device_id: me.device_id.clone(),
                            display_name: me.display_name.clone(),
                            app_version: env!("CARGO_PKG_VERSION").to_string(),
                            platform: me.platform.clone(),
                            capabilities: caps.clone(),
                            nonce: rand::random(),
                            peer_nonce: nonce,
                        },
                    ),
                )
                .await?;
                (
                    device_id,
                    protocol::sanitize_remote_text(&display_name, protocol::MAX_DISPLAY_NAME_CHARS),
                    protocol::sanitize_remote_text(&platform, 32),
                    agreed,
                )
            }
            other => {
                return Err(Error::MalformedMessage(format!(
                    "expected Hello, got {}",
                    message_name(&other)
                )))
            }
        };
    let chunk_size = agreed.max_chunk_size;

    // --- request ----------------------------------------------------------------
    let (transfer_id, total_bytes, file_count, entries) =
        match timed(opts.handshake_timeout, read_message(&mut stream)).await? {
            Message::TransferRequest {
                transfer_id,
                total_bytes,
                file_count,
                entries,
            } => (transfer_id, total_bytes, file_count, entries),
            other => {
                return Err(Error::MalformedMessage(format!(
                    "expected TransferRequest, got {}",
                    message_name(&other)
                )))
            }
        };

    let targets = match plan_targets(&transfer_id, &entries, total_bytes, file_count, config).await
    {
        Ok(t) => t,
        Err(e) => {
            let _ = write_message(
                &mut stream,
                &Message::Error {
                    transfer_id: filesystem::is_valid_transfer_id(&transfer_id)
                        .then(|| transfer_id.clone()),
                    code: e.classify().kind.to_string(),
                    detail: String::new(),
                },
            )
            .await;
            warn!(error = %e, "refused an incoming transfer request");
            return Err(e);
        }
    };

    // --- approval or resume -----------------------------------------------------
    let print = fingerprint(&entries, chunk_size);
    let existing = registry
        .get(&transfer_id)
        .filter(|s| s.peer_device_id == peer_id && s.fingerprint == print);
    let control = Control::new();
    let request = IncomingRequest {
        transfer_id: transfer_id.clone(),
        peer_device_id: peer_id.clone(),
        peer_name: peer_name.clone(),
        peer_platform,
        file_count,
        total_bytes,
        top_level: top_level_names(&entries),
        destination: config.destination.clone(),
        resuming: existing.is_some(),
    };

    let session = match existing {
        Some(session) => {
            handler.resumed(request, control.clone());
            session
        }
        None => {
            let decision = tokio::time::timeout(
                opts.decision_timeout,
                handler.decide(request, control.clone()),
            )
            .await
            .unwrap_or_else(|_| Decision::Reject("No answer from the receiving user.".into()));
            if let Decision::Reject(reason) = decision {
                let _ = write_message(
                    &mut stream,
                    &Message::Reject {
                        transfer_id: transfer_id.clone(),
                        reason,
                    },
                )
                .await;
                return Ok(ReceiveReport {
                    transfer_id,
                    outcome: ReceiveOutcome::Rejected,
                    bytes: 0,
                    files: Vec::new(),
                    skipped: 0,
                });
            }
            let session = Arc::new(ReceiveSession {
                peer_device_id: peer_id.clone(),
                fingerprint: print,
                completed: Mutex::new(HashSet::new()),
            });
            registry.insert(transfer_id.clone(), session.clone());
            session
        }
    };

    // --- accept -----------------------------------------------------------------
    if let Err(e) = create_directories(&targets, &config.destination).await {
        let _ = write_message(
            &mut stream,
            &Message::Error {
                transfer_id: Some(transfer_id.clone()),
                code: e.classify().kind.to_string(),
                detail: String::new(),
            },
        )
        .await;
        return Err(e);
    }

    let sizes: Vec<u64> = targets
        .iter()
        .map(|t| if t.entry.is_dir { 0 } else { t.entry.size })
        .collect();
    let completed = session.completed();
    let mut expected: HashSet<(u32, u64)> = HashSet::new();
    for (i, size) in sizes.iter().enumerate() {
        for c in 0..chunk_count(*size, chunk_size) {
            if !completed.contains(&(i as u32, c)) {
                expected.insert((i as u32, c));
            }
        }
    }
    let already: u64 = completed
        .iter()
        .filter(|(f, _)| (*f as usize) < sizes.len())
        .map(|(f, c)| u64::from(chunk_len(sizes[*f as usize], chunk_size, *c)))
        .sum();

    timed(
        opts.handshake_timeout,
        write_message(
            &mut stream,
            &Message::Accept {
                transfer_id: transfer_id.clone(),
                completed_chunks: completed
                    .iter()
                    .map(|(f, c)| ChunkRef {
                        file_index: *f,
                        chunk_index: *c,
                    })
                    .collect(),
            },
        ),
    )
    .await?;

    let mut reporter = Reporter::new(
        progress,
        TransferProgress::new(
            transfer_id.clone(),
            TransferDirection::Receive,
            peer_name,
            total_bytes,
        ),
    );
    reporter.baseline(already);
    reporter.status(TransferStatus::Transferring);

    // --- data -------------------------------------------------------------------
    let parts: Arc<Vec<Option<PathBuf>>> =
        Arc::new(targets.iter().map(|t| t.part.clone()).collect());
    let (tx, rx) = mpsc::channel::<ChunkBuffer>(PIPELINE_DEPTH);
    let writer = tokio::spawn(write_chunks(
        rx,
        parts,
        Arc::new(sizes.clone()),
        session.clone(),
    ));
    let mut received = already;

    let end = loop {
        let next = tokio::select! {
            biased;
            _ = control.stopped() => break LoopEnd::LocalStop(control.reason()),
            m = timed(opts.idle_timeout, read_message(&mut stream)) => m,
        };
        let header = match next {
            Ok(Message::ChunkHeader {
                transfer_id: t,
                file_index,
                chunk_index,
                size,
            }) => {
                let valid = t == transfer_id
                    && (file_index as usize) < targets.len()
                    && !targets[file_index as usize].entry.is_dir
                    && size == chunk_len(sizes[file_index as usize], chunk_size, chunk_index)
                    && size > 0
                    && expected.remove(&(file_index, chunk_index));
                if !valid {
                    break LoopEnd::Protocol(Error::MalformedMessage(format!(
                        "unexpected chunk {chunk_index} of file {file_index} ({size} bytes)"
                    )));
                }
                (file_index, chunk_index, size)
            }
            Ok(Message::TransferComplete {
                transfer_id: t,
                digests,
            }) if t == transfer_id => break LoopEnd::Complete(digests),
            Ok(Message::Error { code, .. }) if code == "cancelled" => {
                break LoopEnd::PeerStop(StopReason::Cancel)
            }
            Ok(Message::Error { code, .. }) if code == "paused" => {
                break LoopEnd::PeerStop(StopReason::Pause)
            }
            Ok(Message::Error { code, detail, .. }) => {
                break LoopEnd::Protocol(peer_error(&code, &detail));
            }
            Ok(other) => {
                break LoopEnd::Protocol(Error::MalformedMessage(format!(
                    "unexpected {} during the transfer",
                    message_name(&other)
                )))
            }
            Err(Error::Io(_)) | Err(Error::ConnectionLost { .. }) => break LoopEnd::Lost,
            Err(e) => break LoopEnd::Protocol(e),
        };

        let (file_index, chunk_index, size) = header;
        let mut data = vec![0u8; size as usize];
        let body = timed(opts.idle_timeout, async {
            stream.read_exact(&mut data).await?;
            read_message(&mut stream).await
        })
        .await;
        let footer_digest = match body {
            Ok(Message::ChunkFooter {
                transfer_id: t,
                file_index: f,
                chunk_index: c,
                sha256,
            }) if t == transfer_id && f == file_index && c == chunk_index => sha256,
            Ok(_) => {
                break LoopEnd::Protocol(Error::MalformedMessage(
                    "chunk footer does not match its header".into(),
                ))
            }
            Err(Error::Io(_)) | Err(Error::ConnectionLost { .. }) => break LoopEnd::Lost,
            Err(e) => break LoopEnd::Protocol(e),
        };

        // Hash off the async runtime: 4 MiB of SHA-256 is milliseconds of CPU.
        let hashed = tokio::task::spawn_blocking(move || {
            let actual: [u8; 32] = Sha256::digest(&data).into();
            (data, actual)
        })
        .await;
        let Ok((data, actual)) = hashed else {
            break LoopEnd::Protocol(Error::Other(anyhow::anyhow!("hashing stopped")));
        };
        if actual != footer_digest {
            break LoopEnd::Corrupt {
                path: targets[file_index as usize].entry.relative_path.clone(),
                expected: hex(&footer_digest),
                actual: hex(&actual),
            };
        }

        let buffer = ChunkBuffer {
            file_index,
            chunk_index,
            offset: chunk_index * u64::from(chunk_size),
            data,
        };
        if tx.send(buffer).await.is_err() {
            break LoopEnd::WriterFailed;
        }
        received += u64::from(size);
        reporter.bytes(
            received,
            Some(&targets[file_index as usize].entry.relative_path),
        );
    };

    // Every queued chunk lands (and is counted) before anything else happens.
    drop(tx);
    let written = match writer.await {
        Ok(r) => r,
        Err(e) => Err(Error::Other(anyhow::anyhow!("writer stopped: {e}"))),
    };

    let digests = match end {
        LoopEnd::Complete(digests) => digests,
        LoopEnd::LocalStop(reason) => {
            send_stop(&mut stream, &transfer_id, stop_code(reason)).await;
            return finish_stopped(
                reason.unwrap_or(StopReason::Cancel),
                &transfer_id,
                &targets,
                &registry,
                received,
            )
            .await;
        }
        LoopEnd::PeerStop(reason) => {
            return finish_stopped(reason, &transfer_id, &targets, &registry, received).await;
        }
        LoopEnd::Lost => {
            debug!(%transfer_id, received, "connection lost; keeping the session for resume");
            return Err(Error::ConnectionLost {
                transferred: received,
                resumable: true,
            });
        }
        LoopEnd::WriterFailed => {
            let e = written
                .err()
                .unwrap_or(Error::Other(anyhow::anyhow!("writer stopped")));
            send_stop(&mut stream, &transfer_id, e.classify().kind).await;
            return Err(e);
        }
        LoopEnd::Protocol(e) => {
            send_stop(&mut stream, &transfer_id, "protocolError").await;
            remove_parts(&targets).await;
            registry.remove(&transfer_id);
            return Err(e);
        }
        LoopEnd::Corrupt {
            path,
            expected,
            actual,
        } => {
            let _ = write_message(
                &mut stream,
                &Message::VerificationFailed {
                    transfer_id: transfer_id.clone(),
                    relative_path: path.clone(),
                    expected: expected.clone(),
                    actual: actual.clone(),
                },
            )
            .await;
            remove_parts(&targets).await;
            registry.remove(&transfer_id);
            return Err(Error::VerificationFailed {
                path,
                expected,
                actual,
            });
        }
    };
    written?;

    if !expected.is_empty() {
        send_stop(&mut stream, &transfer_id, "protocolError").await;
        remove_parts(&targets).await;
        registry.remove(&transfer_id);
        return Err(Error::MalformedMessage(format!(
            "the sender finished with {} chunks still missing",
            expected.len()
        )));
    }

    // --- verify: re-read every part file from disk (D-F06-3) -------------------
    reporter.status(TransferStatus::Verifying);
    for (i, t) in targets.iter().enumerate() {
        let Some(part) = &t.part else {
            continue;
        };
        if t.entry.size == 0 {
            tokio::fs::File::create(part).await?;
        }
        let actual = hash_file(part.clone()).await?;
        let declared = digests
            .iter()
            .find(|d| d.file_index as usize == i)
            .map(|d| d.sha256.to_lowercase());
        let matches_declared = match &t.entry.sha256 {
            Some(s) => s.to_lowercase() == actual,
            None => true,
        };
        if declared.as_deref() != Some(actual.as_str()) || !matches_declared {
            let expected = declared.unwrap_or_default();
            let _ = write_message(
                &mut stream,
                &Message::VerificationFailed {
                    transfer_id: transfer_id.clone(),
                    relative_path: t.entry.relative_path.clone(),
                    expected: expected.clone(),
                    actual: actual.clone(),
                },
            )
            .await;
            remove_parts(&targets).await;
            registry.remove(&transfer_id);
            return Err(Error::VerificationFailed {
                path: t.entry.relative_path.clone(),
                expected,
                actual,
            });
        }
    }

    // --- finalise -----------------------------------------------------------------
    let mut files = Vec::new();
    let mut skipped = 0;
    for t in &targets {
        let Some(part) = &t.part else {
            continue;
        };
        let (part, destination, conflict, modified) = (
            part.clone(),
            t.destination.clone(),
            config.conflict,
            t.entry.modified,
        );
        let finalized = tokio::task::spawn_blocking(move || {
            let result = filesystem::finalize_file(&part, &destination, conflict);
            if let Ok(Finalized::Written(path)) = &result {
                filesystem::apply_modified(path, modified);
            }
            result
        })
        .await
        .map_err(|e| Error::Other(anyhow::anyhow!("finalising stopped: {e}")))??;
        match finalized {
            Finalized::Written(path) => files.push(path),
            Finalized::Skipped => skipped += 1,
        }
    }
    registry.remove(&transfer_id);
    let _ = write_message(
        &mut stream,
        &Message::Verified {
            transfer_id: transfer_id.clone(),
        },
    )
    .await;
    reporter.bytes(received, None);
    reporter.status(TransferStatus::Completed);
    info!(%transfer_id, files = files.len(), skipped, "transfer received and verified");

    Ok(ReceiveReport {
        transfer_id,
        outcome: ReceiveOutcome::Completed,
        bytes: received,
        files,
        skipped,
    })
}

/// Ends a stopped receive: a pause keeps everything for resume; a cancel
/// removes the part files and forgets the session.
async fn finish_stopped(
    reason: StopReason,
    transfer_id: &str,
    targets: &[Target],
    registry: &ReceiverRegistry,
    received: u64,
) -> Result<ReceiveReport> {
    match reason {
        StopReason::Pause => Ok(ReceiveReport {
            transfer_id: transfer_id.to_string(),
            outcome: ReceiveOutcome::Paused,
            bytes: received,
            files: Vec::new(),
            skipped: 0,
        }),
        StopReason::Cancel => {
            remove_parts(targets).await;
            registry.remove(transfer_id);
            Err(Error::Cancelled)
        }
    }
}

/// Validates a request and resolves every destination, before the user is
/// asked: a request that is malformed or unsafe never reaches the dialog.
async fn plan_targets(
    transfer_id: &str,
    entries: &[FileEntry],
    total_bytes: u64,
    file_count: u64,
    config: &ReceiverConfig,
) -> Result<Vec<Target>> {
    if !filesystem::is_valid_transfer_id(transfer_id) {
        return Err(Error::MalformedMessage("transfer id is malformed".into()));
    }
    transfer::check_totals(entries, total_bytes, file_count)?;

    let root = config.destination.clone();
    let entries = entries.to_vec();
    let transfer_id = transfer_id.to_string();
    tokio::task::spawn_blocking(move || -> Result<Vec<Target>> {
        std::fs::create_dir_all(&root).map_err(|e| Error::DestinationNotWritable {
            path: root.display().to_string(),
            remedy: format!("Choose another destination folder ({e})."),
        })?;
        filesystem::check_destination_root(&root.canonicalize()?)?;
        let prepared = transfer::prepare(&entries, &root)?;
        prepared
            .into_iter()
            .zip(entries)
            .map(|(p, entry)| {
                let part = if p.is_dir {
                    None
                } else {
                    Some(filesystem::part_path(&p.destination, &transfer_id)?)
                };
                Ok(Target {
                    entry,
                    destination: p.destination,
                    part,
                })
            })
            .collect()
    })
    .await
    .map_err(|e| Error::Other(anyhow::anyhow!("planning stopped: {e}")))?
}

/// Creates every directory the transfer needs, proving containment each time.
async fn create_directories(targets: &[Target], root: &Path) -> Result<()> {
    let plan: Vec<(PathBuf, bool)> = targets
        .iter()
        .map(|t| (t.destination.clone(), t.entry.is_dir))
        .collect();
    let root = root.to_path_buf();
    tokio::task::spawn_blocking(move || -> Result<()> {
        for (destination, is_dir) in plan {
            filesystem::create_parent_dirs(&root, &destination)?;
            if is_dir {
                std::fs::create_dir_all(&destination)?;
                filesystem::verify_within(&root, &destination)?;
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| Error::Other(anyhow::anyhow!("creating folders stopped: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filesystem::{build_manifest, ConflictResolution, LocalItem};
    use crate::protocol::{
        write_message, Capabilities, FileEntry, Message, ProtocolVersion, PROTOCOL_VERSION,
    };
    use crate::transfer::{TransferDirection, TransferProgress, TransferStatus};
    use sha2::{Digest, Sha256};
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex as StdMutex};
    use std::task::{Context, Poll};
    use std::time::Duration;
    use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadBuf};

    const CHUNK: u32 = 64 * 1024;
    const TID: &str = "3f2b8c1e-0000-4000-8000-000000000001";

    fn me(id: &str) -> LocalIdentity {
        LocalIdentity {
            device_id: id.into(),
            display_name: format!("{id} name"),
            platform: "test".into(),
        }
    }

    fn opts() -> SessionOptions {
        SessionOptions {
            max_chunk_size: CHUNK,
            handshake_timeout: Duration::from_secs(5),
            decision_timeout: Duration::from_secs(5),
            idle_timeout: Duration::from_secs(5),
            ..SessionOptions::default()
        }
    }

    fn quiet() -> ProgressSink {
        Arc::new(|_| {})
    }

    type Log = Arc<StdMutex<Vec<TransferProgress>>>;

    fn recorder() -> (ProgressSink, Log) {
        let log: Log = Arc::new(StdMutex::new(Vec::new()));
        let sink_log = log.clone();
        (Arc::new(move |p| sink_log.lock().unwrap().push(p)), log)
    }

    /// A receiving user, scripted.
    struct Handler {
        decision: Decision,
        delay: Duration,
        calls: StdMutex<Vec<IncomingRequest>>,
        control: StdMutex<Option<Control>>,
    }

    impl Handler {
        fn with(decision: Decision, delay: Duration) -> Arc<Self> {
            Arc::new(Self {
                decision,
                delay,
                calls: StdMutex::new(Vec::new()),
                control: StdMutex::new(None),
            })
        }
        fn accept() -> Arc<Self> {
            Self::with(Decision::Accept, Duration::ZERO)
        }
        fn reject() -> Arc<Self> {
            Self::with(Decision::Reject("not now".into()), Duration::ZERO)
        }
        fn silent() -> Arc<Self> {
            Self::with(Decision::Accept, Duration::from_secs(3600))
        }
        fn calls(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    #[async_trait::async_trait]
    impl IncomingHandler for Handler {
        async fn decide(&self, request: IncomingRequest, control: Control) -> Decision {
            self.calls.lock().unwrap().push(request);
            *self.control.lock().unwrap() = Some(control);
            tokio::time::sleep(self.delay).await;
            self.decision.clone()
        }
    }

    fn write_file(path: &Path, len: usize, seed: u8) -> Vec<u8> {
        let bytes: Vec<u8> = (0..len)
            .map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed))
            .collect();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &bytes).unwrap();
        bytes
    }

    fn manifest(paths: &[PathBuf]) -> Vec<LocalItem> {
        build_manifest(paths).unwrap()
    }

    fn job(items: Vec<LocalItem>) -> SendJob {
        SendJob {
            transfer_id: TID.into(),
            items,
        }
    }

    fn rconfig(dest: &Path) -> ReceiverConfig {
        ReceiverConfig {
            destination: dest.to_path_buf(),
            conflict: ConflictResolution::KeepBoth,
        }
    }

    /// Every path under `root`, relative and with `/`, sorted.
    fn tree(root: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for e in entries.flatten() {
                let p = e.path();
                let rel = p
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push(rel);
                if p.is_dir() {
                    walk(base, &p, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(root, root, &mut out);
        out.sort();
        out
    }

    async fn run_pair(
        items: Vec<LocalItem>,
        dest: &Path,
        handler: Arc<Handler>,
        registry: Arc<ReceiverRegistry>,
        send_control: Control,
        send_sink: ProgressSink,
        recv_sink: ProgressSink,
    ) -> (Result<SendReport>, Result<ReceiveReport>) {
        let (a, b) = tokio::io::duplex(1 << 20);
        let (sender_id, receiver_id, options, config) =
            (me("dev-a"), me("dev-b"), opts(), rconfig(dest));
        let send = run_sender(a, &sender_id, &options, job(items), send_control, send_sink);
        let recv = run_receiver(
            b,
            &receiver_id,
            &options,
            &config,
            handler,
            registry,
            recv_sink,
        );
        tokio::join!(send, recv)
    }

    async fn simple(
        items: Vec<LocalItem>,
        dest: &Path,
        handler: Arc<Handler>,
    ) -> (Result<SendReport>, Result<ReceiveReport>) {
        run_pair(
            items,
            dest,
            handler,
            Arc::new(ReceiverRegistry::default()),
            Control::new(),
            quiet(),
            quiet(),
        )
        .await
    }

    // --- F05: transfer -------------------------------------------------------

    /// AC-F05-01
    #[tokio::test]
    async fn single_file_round_trip() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let bytes = write_file(&src.path().join("data.bin"), 300 * 1024 + 7, 1);

        let (sent, received) = simple(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            Handler::accept(),
        )
        .await;
        let sent = sent.unwrap();
        let received = received.unwrap();

        assert_eq!(received.outcome, ReceiveOutcome::Completed);
        assert_eq!(std::fs::read(dest.path().join("data.bin")).unwrap(), bytes);
        assert_eq!(sent.chunks_sent, 5, "300 KiB + 7 B in 64 KiB chunks");
        assert_eq!(
            tree(dest.path()),
            vec!["data.bin"],
            "no part file may remain"
        );
    }

    /// AC-F05-02
    #[tokio::test]
    async fn folder_tree_round_trip() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let root = src.path().join("project");
        let deep = write_file(&root.join("a/b/deep.bin"), 150 * 1024, 2);
        let top = write_file(&root.join("top.txt"), 10, 3);
        write_file(&root.join("zero.bin"), 0, 4);
        std::fs::create_dir_all(root.join("empty")).unwrap();

        let (sent, received) = simple(manifest(&[root]), dest.path(), Handler::accept()).await;
        sent.unwrap();
        assert_eq!(received.unwrap().outcome, ReceiveOutcome::Completed);

        assert_eq!(
            tree(dest.path()),
            vec![
                "project",
                "project/a",
                "project/a/b",
                "project/a/b/deep.bin",
                "project/empty",
                "project/top.txt",
                "project/zero.bin",
            ]
        );
        assert_eq!(
            std::fs::read(dest.path().join("project/a/b/deep.bin")).unwrap(),
            deep
        );
        assert_eq!(
            std::fs::read(dest.path().join("project/top.txt")).unwrap(),
            top
        );
        assert!(std::fs::read(dest.path().join("project/zero.bin"))
            .unwrap()
            .is_empty());
        assert!(dest.path().join("project/empty").is_dir());
    }

    /// AC-F05-09, AC-F04-10, AC-F08-14
    #[tokio::test]
    async fn rejected_request_writes_nothing() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 100_000, 5);
        let handler = Handler::reject();

        let (sent, received) = simple(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            handler.clone(),
        )
        .await;

        assert!(matches!(sent, Err(Error::PeerRejected)), "got {sent:?}");
        assert_eq!(received.unwrap().outcome, ReceiveOutcome::Rejected);
        assert!(tree(dest.path()).is_empty(), "nothing may be written");

        // The receiver was asked exactly once, and saw what it was approving.
        assert_eq!(handler.calls(), 1);
        let call = &handler.calls.lock().unwrap()[0];
        assert_eq!(call.file_count, 1);
        assert_eq!(call.total_bytes, 100_000);
        assert_eq!(call.top_level, vec!["data.bin"]);
        assert_eq!(call.peer_name, "dev-a name");
        assert!(!call.resuming);
    }

    /// AC-F05-05
    #[tokio::test]
    async fn progress_is_monotonic_and_complete() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 400 * 1024, 6);
        let (send_sink, send_log) = recorder();
        let (recv_sink, recv_log) = recorder();

        let (sent, received) = run_pair(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            Handler::accept(),
            Arc::new(ReceiverRegistry::default()),
            Control::new(),
            send_sink,
            recv_sink,
        )
        .await;
        sent.unwrap();
        received.unwrap();

        for (log, direction) in [
            (send_log, TransferDirection::Send),
            (recv_log, TransferDirection::Receive),
        ] {
            let log = log.lock().unwrap();
            assert!(!log.is_empty());
            let mut last = 0;
            for p in log.iter() {
                assert!(
                    p.transferred_bytes >= last,
                    "{direction:?} progress went backwards"
                );
                last = p.transferred_bytes;
                assert_eq!(p.direction, direction);
                assert!(p.fraction().is_finite());
            }
            let end = log.last().unwrap();
            assert_eq!(end.status, TransferStatus::Completed, "{direction:?}");
            assert_eq!(end.transferred_bytes, 400 * 1024);
            assert_eq!(end.total_bytes, 400 * 1024);
        }
    }

    /// AC-F05-06: the sender cancels; nothing is left on the receiver.
    #[tokio::test]
    async fn sender_cancel_cleans_up() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 10 * CHUNK as usize, 7);
        let control = Control::new();
        let stopper = control.clone();
        let sink: ProgressSink = Arc::new(move |p| {
            if p.transferred_bytes > 0 {
                stopper.stop(StopReason::Cancel);
            }
        });

        let (sent, received) = run_pair(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            Handler::accept(),
            Arc::new(ReceiverRegistry::default()),
            control,
            sink,
            quiet(),
        )
        .await;

        assert!(matches!(sent, Err(Error::Cancelled)), "sender: {sent:?}");
        assert!(
            matches!(received, Err(Error::Cancelled)),
            "receiver: {received:?}"
        );
        assert!(
            tree(dest.path()).is_empty(),
            "left behind: {:?}",
            tree(dest.path())
        );
    }

    /// AC-F05-06: the receiver cancels; the sender stops.
    #[tokio::test]
    async fn receiver_cancel_stops_sender() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 10 * CHUNK as usize, 8);
        let handler = Handler::accept();
        let h = handler.clone();
        let sink: ProgressSink = Arc::new(move |p| {
            if p.transferred_bytes > 0 {
                if let Some(c) = h.control.lock().unwrap().as_ref() {
                    c.stop(StopReason::Cancel);
                }
            }
        });

        let (sent, received) = run_pair(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            handler,
            Arc::new(ReceiverRegistry::default()),
            Control::new(),
            quiet(),
            sink,
        )
        .await;

        assert!(
            matches!(received, Err(Error::Cancelled)),
            "receiver: {received:?}"
        );
        assert!(sent.is_err(), "sender must stop: {sent:?}");
        assert!(
            tree(dest.path()).is_empty(),
            "left behind: {:?}",
            tree(dest.path())
        );
    }

    /// AC-F05-07, AC-F06-05, AC-F06-08: pause keeps part files under their
    /// part names; resume sends only what is missing.
    #[tokio::test]
    async fn pause_then_resume_completes() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let bytes = write_file(&src.path().join("data.bin"), 8 * CHUNK as usize, 9);
        let items = manifest(&[src.path().join("data.bin")]);
        let registry = Arc::new(ReceiverRegistry::default());

        let control = Control::new();
        let stopper = control.clone();
        let sink: ProgressSink = Arc::new(move |p| {
            if p.transferred_bytes > 0 {
                stopper.stop(StopReason::Pause);
            }
        });
        let (sent, received) = run_pair(
            items.clone(),
            dest.path(),
            Handler::accept(),
            registry.clone(),
            control,
            sink,
            quiet(),
        )
        .await;
        assert!(
            matches!(sent, Err(Error::Cancelled)),
            "paused sender: {sent:?}"
        );
        assert_eq!(received.unwrap().outcome, ReceiveOutcome::Paused);

        // AC-F06-08: only a part file exists; nothing under the final name.
        let after_pause = tree(dest.path());
        assert_eq!(after_pause.len(), 1, "{after_pause:?}");
        assert!(after_pause[0].ends_with(".ftpart"), "{after_pause:?}");
        let done_before = registry
            .completed_chunks(TID)
            .expect("session kept for resume");
        assert!((1..8).contains(&done_before), "completed {done_before}");

        // Resume: same transfer id, same registry. No second approval needed.
        let handler = Handler::accept();
        let (sent, received) = run_pair(
            items,
            dest.path(),
            handler.clone(),
            registry.clone(),
            Control::new(),
            quiet(),
            quiet(),
        )
        .await;
        let sent = sent.unwrap();
        assert_eq!(received.unwrap().outcome, ReceiveOutcome::Completed);
        assert_eq!(
            handler.calls(),
            0,
            "an in-process resume is not re-approved (D-F06-4)"
        );
        assert_eq!(sent.chunks_sent as usize, 8 - done_before, "AC-F06-05");
        assert_eq!(std::fs::read(dest.path().join("data.bin")).unwrap(), bytes);
        assert_eq!(tree(dest.path()), vec!["data.bin"]);
        assert!(
            registry.completed_chunks(TID).is_none(),
            "session cleared when done"
        );
    }

    /// D-F06-4: the same transfer id with a different manifest is a new request.
    #[tokio::test]
    async fn resume_with_a_different_manifest_is_a_new_request() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 4 * CHUNK as usize, 10);
        let registry = Arc::new(ReceiverRegistry::default());
        let control = Control::new();
        let stopper = control.clone();
        let sink: ProgressSink = Arc::new(move |p| {
            if p.transferred_bytes > 0 {
                stopper.stop(StopReason::Pause);
            }
        });
        let _ = run_pair(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            Handler::accept(),
            registry.clone(),
            control,
            sink,
            quiet(),
        )
        .await;

        write_file(&src.path().join("data.bin"), 5 * CHUNK as usize, 11);
        let handler = Handler::accept();
        let (sent, received) = run_pair(
            manifest(&[src.path().join("data.bin")]),
            dest.path(),
            handler.clone(),
            registry,
            Control::new(),
            quiet(),
            quiet(),
        )
        .await;
        sent.unwrap();
        received.unwrap();
        assert_eq!(
            handler.calls(),
            1,
            "a changed manifest must be approved again"
        );
        assert!(!handler.calls.lock().unwrap()[0].resuming);
    }

    // --- F06: integrity under faults -----------------------------------------

    /// Writes pass through until `remaining` bytes, then the "cable" is cut.
    struct CutAfter<S> {
        inner: S,
        remaining: usize,
    }

    impl<S: AsyncRead + Unpin> AsyncRead for CutAfter<S> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl<S: AsyncWrite + Unpin> AsyncWrite for CutAfter<S> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            if self.remaining == 0 {
                return Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()));
            }
            let n = buf.len().min(self.remaining);
            match Pin::new(&mut self.inner).poll_write(cx, &buf[..n]) {
                Poll::Ready(Ok(written)) => {
                    self.remaining -= written;
                    Poll::Ready(Ok(written))
                }
                other => other,
            }
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    /// Reads pass through, except one bit of the byte at stream offset `at`.
    struct FlipByteAt<S> {
        inner: S,
        at: u64,
        pos: u64,
    }

    impl<S: AsyncRead + Unpin> AsyncRead for FlipByteAt<S> {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let before = buf.filled().len();
            let result = Pin::new(&mut self.inner).poll_read(cx, buf);
            if let Poll::Ready(Ok(())) = &result {
                let got = (buf.filled().len() - before) as u64;
                let (at, pos) = (self.at, self.pos);
                if at >= pos && at < pos + got {
                    buf.filled_mut()[before + (at - pos) as usize] ^= 0x01;
                }
                self.pos += got;
            }
            result
        }
    }

    impl<S: AsyncWrite + Unpin> AsyncWrite for FlipByteAt<S> {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }
        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }
        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            Pin::new(&mut self.inner).poll_shutdown(cx)
        }
    }

    /// AC-F06-04, AC-F01-08: a cable cut mid-chunk counts only whole chunks,
    /// ends `INTERRUPTED`-style (connection lost), and resumes cleanly.
    #[tokio::test]
    async fn partial_chunk_is_never_counted() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let bytes = write_file(&src.path().join("data.bin"), 6 * CHUNK as usize, 12);
        let items = manifest(&[src.path().join("data.bin")]);
        let registry = Arc::new(ReceiverRegistry::default());

        // Handshake and one whole chunk get through; the second chunk is cut
        // in half.
        let (a, b) = tokio::io::duplex(1 << 20);
        let cut = CutAfter {
            inner: a,
            remaining: CHUNK as usize + CHUNK as usize / 2 + 2_000,
        };
        let (sender_id, receiver_id, options, config) =
            (me("dev-a"), me("dev-b"), opts(), rconfig(dest.path()));
        let send = run_sender(
            cut,
            &sender_id,
            &options,
            job(items.clone()),
            Control::new(),
            quiet(),
        );
        let recv = run_receiver(
            b,
            &receiver_id,
            &options,
            &config,
            Handler::accept(),
            registry.clone(),
            quiet(),
        );
        let (sent, received) = tokio::join!(send, recv);
        assert!(
            matches!(
                sent,
                Err(Error::ConnectionLost {
                    resumable: true,
                    ..
                })
            ),
            "sender: {sent:?}"
        );
        assert!(
            matches!(
                received,
                Err(Error::ConnectionLost {
                    resumable: true,
                    ..
                })
            ),
            "receiver: {received:?}"
        );
        assert_eq!(
            registry.completed_chunks(TID),
            Some(1),
            "only the whole chunk counts"
        );
        assert!(!dest.path().join("data.bin").exists());

        let (sent, received) = run_pair(
            items,
            dest.path(),
            Handler::accept(),
            registry,
            Control::new(),
            quiet(),
            quiet(),
        )
        .await;
        assert_eq!(sent.unwrap().chunks_sent, 5);
        assert_eq!(received.unwrap().outcome, ReceiveOutcome::Completed);
        assert_eq!(std::fs::read(dest.path().join("data.bin")).unwrap(), bytes);
    }

    /// AC-F06-06: one flipped bit in transit fails the transfer; nothing is
    /// finalised.
    #[tokio::test]
    async fn tampered_chunk_is_detected() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 5 * CHUNK as usize, 13);

        let (a, b) = tokio::io::duplex(1 << 20);
        let tampered = FlipByteAt {
            inner: b,
            at: 200_000,
            pos: 0,
        };
        let (sender_id, receiver_id, options, config) =
            (me("dev-a"), me("dev-b"), opts(), rconfig(dest.path()));
        let send = run_sender(
            a,
            &sender_id,
            &options,
            job(manifest(&[src.path().join("data.bin")])),
            Control::new(),
            quiet(),
        );
        let recv = run_receiver(
            tampered,
            &receiver_id,
            &options,
            &config,
            Handler::accept(),
            Arc::new(ReceiverRegistry::default()),
            quiet(),
        );
        let (sent, received) = tokio::join!(send, recv);
        assert!(
            matches!(
                received,
                Err(Error::VerificationFailed { .. }) | Err(Error::MalformedMessage(_))
            ),
            "receiver: {received:?}"
        );
        assert!(sent.is_err());
        assert!(
            !dest.path().join("data.bin").exists(),
            "a corrupt file must never be finalised"
        );
    }

    // --- scripted peers -------------------------------------------------------

    fn hello(major: u16) -> Message {
        Message::Hello {
            device_id: "evil-peer".into(),
            display_name: "Evil".into(),
            app_version: "0.0.0".into(),
            platform: "test".into(),
            capabilities: Capabilities {
                protocol: ProtocolVersion { major, minor: 0 },
                max_chunk_size: CHUNK,
                ..Capabilities::default()
            },
            nonce: [7; 32],
        }
    }

    fn file_entry(path: &str, size: u64) -> FileEntry {
        FileEntry {
            relative_path: path.into(),
            size,
            is_dir: false,
            mode: None,
            modified: None,
            sha256: None,
        }
    }

    /// Speaks the protocol by hand, up to and including the receiver's answer
    /// to a `TransferRequest`.
    async fn scripted_request(stream: &mut DuplexStream, entries: Vec<FileEntry>) -> Message {
        write_message(stream, &hello(PROTOCOL_VERSION.major))
            .await
            .unwrap();
        let ack = read_message(stream).await.unwrap();
        assert!(matches!(ack, Message::HelloAck { .. }), "got {ack:?}");
        let total = entries.iter().map(|e| e.size).sum();
        let count = entries.iter().filter(|e| !e.is_dir).count() as u64;
        write_message(
            stream,
            &Message::TransferRequest {
                transfer_id: TID.into(),
                total_bytes: total,
                file_count: count,
                entries,
            },
        )
        .await
        .unwrap();
        read_message(stream).await.unwrap()
    }

    fn spawn_receiver(
        stream: DuplexStream,
        dest: &Path,
    ) -> tokio::task::JoinHandle<Result<ReceiveReport>> {
        let dest = dest.to_path_buf();
        tokio::spawn(async move {
            run_receiver(
                stream,
                &me("dev-b"),
                &opts(),
                &rconfig(&dest),
                Handler::accept(),
                Arc::new(ReceiverRegistry::default()),
                quiet(),
            )
            .await
        })
    }

    /// AC-F06-07: a sender whose end-to-end digest is wrong fails verification;
    /// no file is finalised and no part file remains.
    #[tokio::test]
    async fn end_to_end_mismatch_fails_the_transfer() {
        let dest = tempfile::tempdir().unwrap();
        let (mut a, b) = tokio::io::duplex(1 << 20);
        let receiver = spawn_receiver(b, dest.path());

        let answer = scripted_request(&mut a, vec![file_entry("note.txt", 10)]).await;
        assert!(matches!(answer, Message::Accept { .. }), "got {answer:?}");
        let data = b"0123456789";
        write_message(
            &mut a,
            &Message::ChunkHeader {
                transfer_id: TID.into(),
                file_index: 0,
                chunk_index: 0,
                size: 10,
            },
        )
        .await
        .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut a, data)
            .await
            .unwrap();
        write_message(
            &mut a,
            &Message::ChunkFooter {
                transfer_id: TID.into(),
                file_index: 0,
                chunk_index: 0,
                sha256: Sha256::digest(data).into(),
            },
        )
        .await
        .unwrap();
        write_message(
            &mut a,
            &Message::TransferComplete {
                transfer_id: TID.into(),
                digests: vec![crate::protocol::FileDigest {
                    file_index: 0,
                    sha256: "00".repeat(32),
                }],
            },
        )
        .await
        .unwrap();

        let verdict = read_message(&mut a).await.unwrap();
        assert!(
            matches!(verdict, Message::VerificationFailed { .. }),
            "got {verdict:?}"
        );
        let result = receiver.await.unwrap();
        assert!(
            matches!(result, Err(Error::VerificationFailed { .. })),
            "got {result:?}"
        );
        assert!(
            tree(dest.path()).is_empty(),
            "left behind: {:?}",
            tree(dest.path())
        );
    }

    /// AC-F04-09: hostile chunk headers end the session and write nothing final.
    #[tokio::test]
    async fn hostile_chunk_headers_are_refused() {
        let cases = [
            (
                "wrong size",
                Message::ChunkHeader {
                    transfer_id: TID.into(),
                    file_index: 0,
                    chunk_index: 0,
                    size: 9,
                },
            ),
            (
                "chunk out of range",
                Message::ChunkHeader {
                    transfer_id: TID.into(),
                    file_index: 0,
                    chunk_index: 5,
                    size: 10,
                },
            ),
            (
                "file out of range",
                Message::ChunkHeader {
                    transfer_id: TID.into(),
                    file_index: 3,
                    chunk_index: 0,
                    size: 10,
                },
            ),
            (
                "foreign transfer",
                Message::ChunkHeader {
                    transfer_id: "aaaaaaaa-0000-4000-8000-000000000009".into(),
                    file_index: 0,
                    chunk_index: 0,
                    size: 10,
                },
            ),
            (
                "not a chunk",
                Message::Accept {
                    transfer_id: TID.into(),
                    completed_chunks: vec![],
                },
            ),
        ];
        for (name, bad) in cases {
            let dest = tempfile::tempdir().unwrap();
            let (mut a, b) = tokio::io::duplex(1 << 20);
            let receiver = spawn_receiver(b, dest.path());
            let answer = scripted_request(&mut a, vec![file_entry("note.txt", 10)]).await;
            assert!(
                matches!(answer, Message::Accept { .. }),
                "{name}: {answer:?}"
            );
            write_message(&mut a, &bad).await.unwrap();

            let result = receiver.await.unwrap();
            assert!(
                matches!(result, Err(Error::MalformedMessage(_))),
                "{name}: {result:?}"
            );
            assert!(!dest.path().join("note.txt").exists(), "{name}");
        }
    }

    /// AC-F07-10: a traversal attempt is refused and nothing escapes the root.
    #[tokio::test]
    async fn hostile_sender_cannot_escape_the_root() {
        let outer = tempfile::tempdir().unwrap();
        let dest = outer.path().join("inbox");
        std::fs::create_dir_all(&dest).unwrap();
        let (mut a, b) = tokio::io::duplex(1 << 20);
        let receiver = spawn_receiver(b, &dest);

        let answer = scripted_request(&mut a, vec![file_entry("../escape.txt", 4)]).await;
        assert!(
            matches!(answer, Message::Reject { .. } | Message::Error { .. }),
            "got {answer:?}"
        );
        let result = receiver.await.unwrap();
        assert!(
            matches!(result, Err(Error::UnsafePath(_))),
            "got {result:?}"
        );
        assert!(!outer.path().join("escape.txt").exists());
        assert!(tree(&dest).is_empty());
    }

    /// AC-F07-14: permission bits from the sender are never applied.
    #[cfg(unix)]
    #[tokio::test]
    async fn permissions_are_not_applied() {
        use std::os::unix::fs::PermissionsExt;
        let dest = tempfile::tempdir().unwrap();
        let (mut a, b) = tokio::io::duplex(1 << 20);
        let receiver = spawn_receiver(b, dest.path());
        let mut entry = file_entry("run.sh", 4);
        entry.mode = Some(0o4777);
        let answer = scripted_request(&mut a, vec![entry]).await;
        assert!(matches!(answer, Message::Accept { .. }));
        let data = b"echo";
        write_message(
            &mut a,
            &Message::ChunkHeader {
                transfer_id: TID.into(),
                file_index: 0,
                chunk_index: 0,
                size: 4,
            },
        )
        .await
        .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut a, data)
            .await
            .unwrap();
        write_message(
            &mut a,
            &Message::ChunkFooter {
                transfer_id: TID.into(),
                file_index: 0,
                chunk_index: 0,
                sha256: Sha256::digest(data).into(),
            },
        )
        .await
        .unwrap();
        let digest = hex(&Sha256::digest(data));
        write_message(
            &mut a,
            &Message::TransferComplete {
                transfer_id: TID.into(),
                digests: vec![crate::protocol::FileDigest {
                    file_index: 0,
                    sha256: digest,
                }],
            },
        )
        .await
        .unwrap();
        assert!(matches!(
            read_message(&mut a).await.unwrap(),
            Message::Verified { .. }
        ));
        receiver.await.unwrap().unwrap();
        let mode = std::fs::metadata(dest.path().join("run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o7111,
            0,
            "no execute or setuid bit may come from the peer: {mode:o}"
        );
    }

    // --- F04: handshake ---------------------------------------------------------

    /// AC-F04-08: both sides learn about a version mismatch.
    #[tokio::test]
    async fn version_mismatch_is_reported_to_both_sides() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 10, 14);
        let (a, b) = tokio::io::duplex(1 << 20);
        let future = SessionOptions {
            protocol: ProtocolVersion { major: 2, minor: 0 },
            ..opts()
        };
        let (sender_id, receiver_id, options, config) =
            (me("dev-a"), me("dev-b"), opts(), rconfig(dest.path()));
        let send = run_sender(
            a,
            &sender_id,
            &future,
            job(manifest(&[src.path().join("data.bin")])),
            Control::new(),
            quiet(),
        );
        let recv = run_receiver(
            b,
            &receiver_id,
            &options,
            &config,
            Handler::accept(),
            Arc::new(ReceiverRegistry::default()),
            quiet(),
        );
        let (sent, received) = tokio::join!(send, recv);
        match sent {
            Err(Error::VersionMismatch { local, remote }) => {
                assert_eq!(local, "2.0");
                assert_eq!(remote, "1.0");
            }
            other => panic!("sender: {other:?}"),
        }
        assert!(
            matches!(received, Err(Error::VersionMismatch { .. })),
            "receiver: {received:?}"
        );
    }

    /// AC-F04-11: an unanswered request is rejected after the decision timeout.
    #[tokio::test]
    async fn unanswered_request_times_out_as_reject() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        write_file(&src.path().join("data.bin"), 10, 15);
        let (a, b) = tokio::io::duplex(1 << 20);
        let impatient = SessionOptions {
            decision_timeout: Duration::from_millis(200),
            ..opts()
        };
        let (sender_id, receiver_id, options, config) =
            (me("dev-a"), me("dev-b"), opts(), rconfig(dest.path()));
        let send = run_sender(
            a,
            &sender_id,
            &options,
            job(manifest(&[src.path().join("data.bin")])),
            Control::new(),
            quiet(),
        );
        let recv = run_receiver(
            b,
            &receiver_id,
            &impatient,
            &config,
            Handler::silent(),
            Arc::new(ReceiverRegistry::default()),
            quiet(),
        );
        let started = std::time::Instant::now();
        let (sent, received) = tokio::join!(send, recv);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(matches!(sent, Err(Error::PeerRejected)), "sender: {sent:?}");
        assert_eq!(received.unwrap().outcome, ReceiveOutcome::Rejected);
        assert!(tree(dest.path()).is_empty());
    }

    /// AC-F04-12: a peer that never speaks is dropped after the handshake timeout.
    #[tokio::test]
    async fn stalled_handshake_times_out() {
        let dest = tempfile::tempdir().unwrap();
        let (_silent_peer, b) = tokio::io::duplex(1 << 20);
        let quick = SessionOptions {
            handshake_timeout: Duration::from_millis(200),
            ..opts()
        };
        let started = std::time::Instant::now();
        let result = run_receiver(
            b,
            &me("dev-b"),
            &quick,
            &rconfig(dest.path()),
            Handler::accept(),
            Arc::new(ReceiverRegistry::default()),
            quiet(),
        )
        .await;
        assert!(result.is_err());
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    // --- F02: connecting --------------------------------------------------------

    /// AC-F02-07: a refused endpoint falls through to the next one.
    #[tokio::test]
    async fn falls_back_to_the_next_endpoint() {
        let dead = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            l.local_addr().unwrap()
        };
        let live = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let live_addr = live.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = live.accept().await;
        });

        let (_, used) = connect_first(&[dead, live_addr]).await.unwrap();
        assert_eq!(used, live_addr);
        assert!(connect_first(&[dead]).await.is_err());
    }

    /// The same engine over a real TCP socket.
    #[tokio::test]
    async fn tcp_loopback_round_trip() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let bytes = write_file(&src.path().join("data.bin"), 1024 * 1024 + 3, 16);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let dest_path = dest.path().to_path_buf();
        let receiver = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            run_receiver(
                stream,
                &me("dev-b"),
                &opts(),
                &rconfig(&dest_path),
                Handler::accept(),
                Arc::new(ReceiverRegistry::default()),
                quiet(),
            )
            .await
        });
        let (stream, _) = connect_first(&[addr]).await.unwrap();
        let sent = run_sender(
            stream,
            &me("dev-a"),
            &opts(),
            job(manifest(&[src.path().join("data.bin")])),
            Control::new(),
            quiet(),
        )
        .await
        .unwrap();
        assert_eq!(
            receiver.await.unwrap().unwrap().outcome,
            ReceiveOutcome::Completed
        );
        assert_eq!(sent.bytes, bytes.len() as u64);
        assert_eq!(std::fs::read(dest.path().join("data.bin")).unwrap(), bytes);
    }

    /// AC-F05-12, the §112 milestone: 1 GiB over loopback TCP, verified, with
    /// measured throughput. Slow and disk-hungry, so run on demand:
    /// `CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true cargo test --release one_gib -- --ignored --nocapture`
    /// (a plain release build refuses plaintext transfers, D-F08-7).
    #[tokio::test]
    #[ignore]
    async fn one_gib_loopback_milestone() {
        let src = tempfile::tempdir().unwrap();
        let dest = tempfile::tempdir().unwrap();
        let path = src.path().join("one-gib.bin");
        {
            use std::io::Write;
            let mut f = std::io::BufWriter::new(std::fs::File::create(&path).unwrap());
            let block: Vec<u8> = (0..1024 * 1024).map(|i: usize| (i % 251) as u8).collect();
            for _ in 0..1024 {
                f.write_all(&block).unwrap();
            }
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let dest_path = dest.path().to_path_buf();
        let receiver = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            run_receiver(
                stream,
                &me("dev-b"),
                &SessionOptions::default(),
                &rconfig(&dest_path),
                Handler::accept(),
                Arc::new(ReceiverRegistry::default()),
                quiet(),
            )
            .await
        });
        let (stream, _) = connect_first(&[addr]).await.unwrap();
        let sent = run_sender(
            stream,
            &me("dev-a"),
            &SessionOptions::default(),
            job(manifest(&[path])),
            Control::new(),
            quiet(),
        )
        .await
        .unwrap();
        receiver.await.unwrap().unwrap();
        println!(
            "1 GiB in {:.2} s = {:.0} MB/s (measured, loopback, SHA-256 verified)",
            sent.elapsed.as_secs_f64(),
            sent.average_bps as f64 / 1e6
        );
    }

    // --- F08 interim gate -------------------------------------------------------

    /// AC-F08-13: release builds refuse plaintext transfers before any I/O.
    /// Runs only under `cargo test --release`.
    #[cfg(not(debug_assertions))]
    #[tokio::test]
    async fn release_builds_refuse_plaintext_transfers() {
        let dest = tempfile::tempdir().unwrap();
        let (a, b) = tokio::io::duplex(1024);
        let sent = run_sender(
            a,
            &me("dev-a"),
            &opts(),
            job(Vec::new()),
            Control::new(),
            quiet(),
        )
        .await;
        assert!(
            matches!(sent, Err(Error::SecureSessionRequired)),
            "got {sent:?}"
        );
        let received = run_receiver(
            b,
            &me("dev-b"),
            &opts(),
            &rconfig(dest.path()),
            Handler::accept(),
            Arc::new(ReceiverRegistry::default()),
            quiet(),
        )
        .await;
        assert!(
            matches!(received, Err(Error::SecureSessionRequired)),
            "got {received:?}"
        );
    }
}
