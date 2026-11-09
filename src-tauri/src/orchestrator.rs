//! Runs transfers for the desktop app: the listener, incoming approvals,
//! outgoing jobs and the events the UI renders.
//!
//! The engine ([`crate::session`]) knows nothing about Tauri; this module is
//! the glue between it and the window. It owns no protocol logic and no path
//! handling: everything that touches the wire or the disk is in `session`,
//! `filesystem` and `transfer`, where it is tested.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};

use crate::discovery::Discovery;
use crate::error::{Error, Result};
use crate::filesystem::{self, ConflictResolution, LocalItem};
use crate::session::{
    self, connect_first, Control, Decision, IncomingHandler, IncomingRequest, LocalIdentity,
    ProgressSink, ReceiveOutcome, ReceiverConfig, ReceiverRegistry, SendJob, SessionOptions,
    StopReason,
};
use crate::transfer::{TransferDirection, TransferProgress, TransferStatus};
use crate::transport;

/// The UI's copy of every transfer, keyed by id. The core is the source of
/// truth (SRS §80); the UI caches what the events tell it.
pub type TransferTable = Arc<Mutex<HashMap<String, TransferProgress>>>;

/// What the receiving user sees in the incoming-transfer dialog (SRS §48,
/// D-F05-1). Mirrored as `IncomingTransfer` in `src/types/index.ts`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IncomingTransfer {
    pub transfer_id: String,
    pub peer_device_id: String,
    pub peer_name: String,
    pub peer_platform: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub top_level: Vec<String>,
    pub destination: String,
}

/// Where received files go and what happens on a name clash.
#[derive(Debug, Clone)]
pub struct ReceivePolicy {
    pub destination: PathBuf,
    pub conflict: ConflictResolution,
}

struct Job {
    control: Control,
    direction: TransferDirection,
    /// For a send: who to and what, so it can be resumed.
    send: Option<(String, Vec<LocalItem>)>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

/// Owns the running transfers.
pub struct Orchestrator {
    app: AppHandle,
    identity: LocalIdentity,
    discovery: Arc<Discovery>,
    registry: Arc<ReceiverRegistry>,
    transfers: TransferTable,
    policy: Arc<Mutex<ReceivePolicy>>,
    jobs: Mutex<HashMap<String, Job>>,
    pending: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
    /// One active transfer per peer; later ones wait as `QUEUED` (D-F05-5).
    peer_queues: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl Orchestrator {
    pub fn new(
        app: AppHandle,
        identity: LocalIdentity,
        discovery: Arc<Discovery>,
        transfers: TransferTable,
        policy: Arc<Mutex<ReceivePolicy>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app,
            identity,
            discovery,
            registry: Arc::new(ReceiverRegistry::default()),
            transfers,
            policy,
            jobs: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
            peer_queues: Mutex::new(HashMap::new()),
        })
    }

    fn emit<P: Serialize + Clone>(&self, event: &str, payload: &P) {
        if let Err(e) = self.app.emit(event, payload) {
            debug!(event, error = %e, "could not emit to the window");
        }
    }

    /// Records a progress update and forwards it to the UI.
    fn record(&self, progress: TransferProgress) {
        lock(&self.transfers).insert(progress.transfer_id.clone(), progress.clone());
        self.emit("transfer_progress", &progress);
    }

    fn sink(self: &Arc<Self>) -> ProgressSink {
        let this = self.clone();
        Arc::new(move |p| this.record(p))
    }

    // --- receiving ------------------------------------------------------------

    /// Binds the transfer listener and accepts connections forever.
    pub async fn listen(self: Arc<Self>, port: u16) -> Result<()> {
        let listeners = transport::bind_listeners(port).await?;
        info!(port, count = listeners.len(), "transfer listener ready");
        for listener in listeners {
            let this = self.clone();
            tokio::spawn(async move {
                loop {
                    match listener.accept().await {
                        Ok((stream, addr)) => {
                            let _ = stream.set_nodelay(true);
                            debug!(%addr, "incoming connection");
                            let this = this.clone();
                            tokio::spawn(async move { this.receive(stream).await });
                        }
                        Err(e) => {
                            warn!(error = %e, "accept failed");
                            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                        }
                    }
                }
            });
        }
        Ok(())
    }

    async fn receive(self: Arc<Self>, stream: tokio::net::TcpStream) {
        let policy = lock(&self.policy).clone();
        let config = ReceiverConfig {
            destination: policy.destination,
            conflict: policy.conflict,
        };
        let handler = Arc::new(AppIncoming {
            orchestrator: self.clone(),
            current: Mutex::new(None),
        });
        let result = session::run_receiver(
            stream,
            &self.identity,
            &SessionOptions::default(),
            &config,
            handler.clone(),
            self.registry.clone(),
            self.sink(),
        )
        .await;

        let Some((transfer_id, control)) = lock(&handler.current).clone() else {
            // Refused before anyone was asked: nothing for the UI to show.
            if let Err(e) = result {
                debug!(error = %e, "incoming connection ended before a request");
            }
            return;
        };
        lock(&self.pending).remove(&transfer_id);
        let outcome = match result {
            Ok(report) => match report.outcome {
                ReceiveOutcome::Completed => Ok(()),
                ReceiveOutcome::Rejected => Err(Error::Cancelled),
                ReceiveOutcome::Paused => {
                    self.finish_with(
                        &transfer_id,
                        TransferStatus::Paused,
                        None,
                        "transfer_paused",
                    );
                    return;
                }
            },
            Err(e) => Err(e),
        };
        self.finish(&transfer_id, outcome, &control);
    }

    /// Answers a pending incoming request (the dialog's buttons).
    pub fn answer(&self, transfer_id: &str, decision: Decision) -> Result<()> {
        let sender = lock(&self.pending)
            .remove(transfer_id)
            .ok_or_else(|| Error::TransferNotFound(transfer_id.to_string()))?;
        // The session may already have timed out; that is not an error here.
        let _ = sender.send(decision);
        Ok(())
    }

    // --- sending --------------------------------------------------------------

    /// Starts sending `paths` to a discovered device. Returns the transfer id.
    pub async fn start_send(
        self: Arc<Self>,
        device_id: String,
        paths: Vec<String>,
    ) -> Result<String> {
        if !session::PLAINTEXT_ALLOWED {
            return Err(Error::SecureSessionRequired);
        }
        let peer = self
            .discovery
            .peers()
            .await
            .into_iter()
            .find(|p| p.device_id == device_id)
            .ok_or(Error::PeerNotFound)?;

        let chosen: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        let items = tokio::task::spawn_blocking(move || filesystem::build_manifest(&chosen))
            .await
            .map_err(|e| Error::Other(anyhow::anyhow!("reading the selection stopped: {e}")))??;
        if items.is_empty() {
            return Err(Error::Io(std::io::Error::other(
                "nothing was selected to send",
            )));
        }
        let total: u64 = items
            .iter()
            .filter(|i| !i.entry.is_dir)
            .map(|i| i.entry.size)
            .sum();

        let transfer_id = uuid::Uuid::new_v4().to_string();
        let progress = TransferProgress::new(
            transfer_id.clone(),
            TransferDirection::Send,
            peer.display_name.clone(),
            total,
        );
        lock(&self.transfers).insert(transfer_id.clone(), progress.clone());
        // The UI must see the queued state immediately, before any I/O.
        self.emit("transfer_started", &progress);

        let control = Control::new();
        lock(&self.jobs).insert(
            transfer_id.clone(),
            Job {
                control: control.clone(),
                direction: TransferDirection::Send,
                send: Some((device_id.clone(), items.clone())),
            },
        );
        tokio::spawn(
            self.clone()
                .run_send(transfer_id.clone(), device_id, items, control),
        );
        Ok(transfer_id)
    }

    async fn run_send(
        self: Arc<Self>,
        transfer_id: String,
        device_id: String,
        items: Vec<LocalItem>,
        control: Control,
    ) {
        let queue = lock(&self.peer_queues)
            .entry(device_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone();
        let _turn = tokio::select! {
            guard = queue.lock() => guard,
            _ = wait_stopped(&control) => {
                self.finish(&transfer_id, Err(Error::Cancelled), &control);
                return;
            }
        };

        // Best transport first (F02, D-F02-4). Endpoints are re-read now, not
        // when the job was queued, so a cable plugged in meanwhile is used.
        let endpoints = self.discovery.endpoints(&device_id).await;
        let addrs: Vec<_> = endpoints.iter().map(|e| e.addr).collect();
        let result = match connect_first(&addrs).await {
            Ok((stream, addr)) => {
                let transport = endpoints
                    .iter()
                    .find(|e| e.addr == addr)
                    .and_then(|e| e.transport);
                info!(%transfer_id, %addr, ?transport, "connected");
                session::run_sender(
                    stream,
                    &self.identity,
                    &SessionOptions::default(),
                    SendJob {
                        transfer_id: transfer_id.clone(),
                        items,
                    },
                    control.clone(),
                    self.sink(),
                )
                .await
                .map(|_| ())
            }
            Err(e) => Err(e),
        };
        self.finish(&transfer_id, result, &control);
    }

    // --- user controls --------------------------------------------------------

    /// Pause or cancel. A transfer with no task running (interrupted, failed,
    /// paused) is cancelled directly.
    pub fn stop(&self, transfer_id: &str, reason: StopReason) -> Result<()> {
        let status = lock(&self.transfers)
            .get(transfer_id)
            .map(|p| p.status)
            .ok_or_else(|| Error::TransferNotFound(transfer_id.to_string()))?;
        let allowed = match reason {
            StopReason::Pause => status.can_pause(),
            StopReason::Cancel => status.can_cancel(),
        };
        if !allowed {
            return Err(Error::InvalidTransferState {
                transfer_id: transfer_id.to_string(),
                status: format!("{status:?}").to_lowercase(),
                attempted: if reason == StopReason::Pause {
                    "paused"
                } else {
                    "cancelled"
                },
            });
        }

        let running = !status.is_resumable();
        if let Some(job) = lock(&self.jobs).get(transfer_id) {
            if running {
                job.control.stop(reason);
                // A pending approval is answered "no" so the session ends now.
                if let Some(tx) = lock(&self.pending).remove(transfer_id) {
                    let _ = tx.send(Decision::Reject("Cancelled by the receiver.".into()));
                }
                return Ok(());
            }
        }
        // Nothing running: a stopped transfer is simply cancelled.
        lock(&self.jobs).remove(transfer_id);
        self.finish_with(
            transfer_id,
            TransferStatus::Cancelled,
            None,
            "transfer_cancelled",
        );
        Ok(())
    }

    /// Resumes a paused or interrupted send. Only the sending computer can
    /// resume; the receiver keeps its part files and waits (D-F06-4).
    pub fn resume(self: &Arc<Self>, transfer_id: &str) -> Result<()> {
        let status = lock(&self.transfers)
            .get(transfer_id)
            .map(|p| p.status)
            .ok_or_else(|| Error::TransferNotFound(transfer_id.to_string()))?;
        let mut jobs = lock(&self.jobs);
        let job = jobs.get_mut(transfer_id);
        let resumable = status.is_resumable()
            && job
                .as_ref()
                .is_some_and(|j| j.direction == TransferDirection::Send && j.send.is_some());
        if !resumable {
            return Err(Error::InvalidTransferState {
                transfer_id: transfer_id.to_string(),
                status: format!("{status:?}").to_lowercase(),
                attempted: "resumed",
            });
        }
        let job = job.expect("checked above");
        let control = Control::new();
        job.control = control.clone();
        let (device_id, items) = job.send.clone().expect("checked above");
        drop(jobs);

        self.finish_with(
            transfer_id,
            TransferStatus::Queued,
            None,
            "transfer_resumed",
        );
        tokio::spawn(
            self.clone()
                .run_send(transfer_id.to_string(), device_id, items, control),
        );
        Ok(())
    }

    // --- endings ---------------------------------------------------------------

    /// Maps how a session ended to a status and an event (D-F06-7).
    fn finish(&self, transfer_id: &str, result: Result<()>, control: &Control) {
        let (status, error, event) = match result {
            Ok(()) => (TransferStatus::Completed, None, "transfer_completed"),
            Err(Error::Cancelled) if control.reason() == Some(StopReason::Pause) => {
                (TransferStatus::Paused, None, "transfer_paused")
            }
            Err(Error::Cancelled) => (TransferStatus::Cancelled, None, "transfer_cancelled"),
            Err(e @ Error::DiskFull { .. }) => (TransferStatus::Paused, Some(e), "transfer_paused"),
            Err(e @ Error::ConnectionLost { .. }) => {
                (TransferStatus::Interrupted, Some(e), "transfer_failed")
            }
            Err(e) => (TransferStatus::Failed, Some(e), "transfer_failed"),
        };
        if let Some(e) = &error {
            warn!(%transfer_id, error = %e, ?status, "transfer ended");
        }
        if status.is_terminal() && status != TransferStatus::Failed {
            lock(&self.jobs).remove(transfer_id);
        }
        self.finish_with(transfer_id, status, error.as_ref(), event);
    }

    fn finish_with(
        &self,
        transfer_id: &str,
        status: TransferStatus,
        error: Option<&Error>,
        event: &str,
    ) {
        let snapshot = {
            let mut table = lock(&self.transfers);
            let Some(p) = table.get_mut(transfer_id) else {
                return;
            };
            p.status = status;
            p.error = error.map(Error::classify);
            p.current_bps = 0;
            p.eta_seconds = None;
            p.clone()
        };
        self.emit(event, &snapshot);
    }
}

async fn wait_stopped(control: &Control) {
    // `Control` exposes no public future, so poll its flag; queueing is rare
    // and this only runs while a transfer waits its turn.
    while !control.is_stopped() {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// The receiving UI for one connection.
struct AppIncoming {
    orchestrator: Arc<Orchestrator>,
    /// The transfer this connection turned out to carry, once known.
    current: Mutex<Option<(String, Control)>>,
}

impl AppIncoming {
    fn register(&self, request: &IncomingRequest, control: &Control) {
        *lock(&self.current) = Some((request.transfer_id.clone(), control.clone()));
        let o = &self.orchestrator;
        let mut progress = TransferProgress::new(
            request.transfer_id.clone(),
            TransferDirection::Receive,
            request.peer_name.clone(),
            request.total_bytes,
        );
        if let Some(previous) = lock(&o.transfers).get(&request.transfer_id) {
            progress.transferred_bytes = previous.transferred_bytes;
        }
        lock(&o.transfers).insert(request.transfer_id.clone(), progress.clone());
        lock(&o.jobs).insert(
            request.transfer_id.clone(),
            Job {
                control: control.clone(),
                direction: TransferDirection::Receive,
                send: None,
            },
        );
        o.emit("transfer_started", &progress);
    }
}

#[async_trait::async_trait]
impl IncomingHandler for AppIncoming {
    async fn decide(&self, request: IncomingRequest, control: Control) -> Decision {
        self.register(&request, &control);
        let (tx, rx) = oneshot::channel();
        lock(&self.orchestrator.pending).insert(request.transfer_id.clone(), tx);
        self.orchestrator.emit(
            "incoming_transfer",
            &IncomingTransfer {
                transfer_id: request.transfer_id.clone(),
                peer_device_id: request.peer_device_id.clone(),
                peer_name: request.peer_name.clone(),
                peer_platform: request.peer_platform.clone(),
                file_count: request.file_count,
                total_bytes: request.total_bytes,
                top_level: request.top_level.clone(),
                destination: request.destination.display().to_string(),
            },
        );
        rx.await
            .unwrap_or_else(|_| Decision::Reject("The request was withdrawn.".into()))
    }

    fn resumed(&self, request: IncomingRequest, control: Control) {
        self.register(&request, &control);
        self.orchestrator.emit(
            "transfer_resumed",
            &lock(&self.orchestrator.transfers)
                .get(&request.transfer_id)
                .cloned(),
        );
    }
}

/// The default download folder: `<Downloads>/FastTransfer` (SRS §45).
pub fn default_download_dir() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.download_dir().map(|p| p.join("FastTransfer")))
        .or_else(|| directories::BaseDirs::new().map(|d| d.home_dir().join("FastTransfer")))
        .unwrap_or_else(|| PathBuf::from("FastTransfer"))
}
