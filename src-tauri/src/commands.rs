//! The IPC surface.
//!
//! Every function here is callable from the frontend as `invoke("<name>", args)`.
//! Implements the command list in SRS §79.
//!
//! # Rules
//!
//! - Commands take and return plain serializable types. No `Box<dyn …>` crosses
//!   this boundary.
//! - Errors are [`crate::error::Error`], which serializes to a structured
//!   payload the UI renders (SRS §95) — never a raw `Debug` string.
//! - Any type here must have a mirror in `src/types/index.ts`. Changing one
//!   without the other is a runtime failure, not a compile error.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::discovery::{Discovery, Peer};
use crate::error::{Error, ErrorPayload, Result};
use crate::link::{self, LinkExplanation, LinkKind};
use crate::orchestrator::{default_download_dir, Orchestrator, ReceivePolicy, TransferTable};
use crate::security::{DeviceIdentity, PairingState, TrustedDevice};
use crate::session::{Decision, LocalIdentity, StopReason};
use crate::transfer::TransferProgress;
use crate::transport::{self, TransportCandidate};

/// Application state shared across commands.
pub struct AppState {
    pub discovery: Arc<Discovery>,
    pub identity: DeviceIdentity,
    pub pairing: Arc<Mutex<PairingState>>,
    pub trusted: Arc<Mutex<Vec<TrustedDevice>>>,
    /// A std mutex: progress updates arrive from synchronous callbacks.
    pub transfers: TransferTable,
    pub settings: Arc<std::sync::Mutex<ApplicationSettings>>,
    pub policy: Arc<std::sync::Mutex<ReceivePolicy>>,
    pub orchestrator: Arc<Orchestrator>,
}

impl AppState {
    pub fn new(app: tauri::AppHandle, discovery: Arc<Discovery>, identity: DeviceIdentity) -> Self {
        let settings = ApplicationSettings::default();
        let policy = Arc::new(std::sync::Mutex::new(ReceivePolicy {
            destination: settings.download_directory.clone().into(),
            conflict: settings.conflict_resolution,
        }));
        let transfers: TransferTable = Arc::new(std::sync::Mutex::new(HashMap::new()));
        let orchestrator = Orchestrator::new(
            app,
            LocalIdentity {
                device_id: identity.device_id.clone(),
                display_name: identity.display_name.clone(),
                platform: identity.platform.clone(),
            },
            discovery.clone(),
            transfers.clone(),
            policy.clone(),
        );
        Self {
            discovery,
            identity,
            pairing: Arc::new(Mutex::new(PairingState::new())),
            trusted: Arc::new(Mutex::new(Vec::new())),
            transfers,
            settings: Arc::new(std::sync::Mutex::new(settings)),
            policy,
            orchestrator,
        }
    }
}

// ---------------------------------------------------------------------------
// Link and transport
// ---------------------------------------------------------------------------

/// What the plugged cable can actually do, and why. SRS §17.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkInfo {
    pub kind: LinkKind,
    pub explanation: LinkExplanation,
    /// Measured throughput of the active link, if a transfer has ever run.
    /// Never the link rating — SRS §101.
    pub measured_bps: Option<u64>,
}

/// Runs a blocking closure off the async runtime.
///
/// The link and interface probes read sysfs or walk the Windows device tree
/// through SetupAPI. Calling them directly from a command either stalls the UI
/// thread (a sync command runs on the main thread) or ties up an async worker.
/// A failed join is a bug in the closure, not a user error, so it surfaces as
/// `Error::Other`.
async fn blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| Error::Other(anyhow::anyhow!("background task failed: {e}")))
}

#[tauri::command]
pub async fn get_link_info() -> Result<LinkInfo> {
    let (kind, explanation) = blocking(link::current).await?;
    Ok(LinkInfo {
        kind,
        explanation,
        measured_bps: None,
    })
}

#[tauri::command]
pub async fn get_transports() -> Result<Vec<TransportCandidate>> {
    blocking(transport::candidates).await
}

#[tauri::command]
pub async fn get_devices(state: State<'_, AppState>) -> Result<Vec<Peer>> {
    Ok(state.discovery.peers().await)
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

/// The diagnostics payload behind Settings > Advanced (SRS §43).
///
/// This exists so a user can answer "why isn't it working" without reading logs.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub device_id: String,
    pub display_name: String,
    pub app_version: String,
    pub platform: String,
    pub link: LinkInfo,
    pub interfaces: Vec<crate::link::Interface>,
    pub transports: Vec<TransportCandidate>,
    pub peer_count: usize,
    pub discovery_working: bool,
    pub trusted_count: usize,
    pub pending_pairings: usize,
    /// Plain-language notes about likely problems. SRS FR-FW-001 in particular:
    /// a Windows machine that classifies a direct cable link as "Public" will
    /// have discovery blocked, and the user needs to be told that specifically.
    pub notes: Vec<String>,
}

#[tauri::command]
pub async fn get_diagnostics(state: State<'_, AppState>) -> Result<Diagnostics> {
    // One closure, one hop to the blocking pool, for everything that touches
    // the OS.
    let ((kind, explanation), transports, interfaces) = blocking(|| {
        (
            link::current(),
            transport::candidates(),
            transport::enumerate_interfaces(),
        )
    })
    .await?;
    let peers = state.discovery.peers().await;

    let mut notes = Vec::new();

    if !kind.is_usable() {
        if let Some(remedy) = &explanation.remedy {
            notes.push(remedy.clone());
        }
    }

    if transports.is_empty() {
        notes.push("No usable network interface was found.".into());
    }

    if peers.is_empty() && !transports.is_empty() {
        notes.push(
            "No other computer has been found yet. Check that FastTransfer is open on both \
             computers and that the firewall allows it."
                .into(),
        );
    }

    Ok(Diagnostics {
        device_id: state.identity.device_id.clone(),
        display_name: state.identity.display_name.clone(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        link: LinkInfo {
            kind,
            explanation,
            measured_bps: None,
        },
        interfaces,
        transports,
        peer_count: peers.len(),
        discovery_working: !peers.is_empty(),
        trusted_count: state.trusted.lock().await.len(),
        pending_pairings: state.pairing.lock().await.len(),
        notes,
    })
}

// ---------------------------------------------------------------------------
// Pairing and trust
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingChallenge {
    pub device_id: String,
    pub display_name: String,
    pub platform: String,
    /// Display this. The user compares it with the other screen and approves
    /// only if they match (SRS FR-PAIR-003).
    pub code: String,
    pub expires_in_seconds: u64,
}

#[tauri::command]
pub async fn pair_device(
    state: State<'_, AppState>,
    device_id: String,
    display_name: String,
    platform: String,
) -> Result<PairingChallenge> {
    let code = state
        .pairing
        .lock()
        .await
        .begin(&device_id, &display_name, &platform);

    Ok(PairingChallenge {
        device_id,
        display_name,
        platform,
        code,
        expires_in_seconds: crate::security::PAIRING_CODE_TTL.as_secs(),
    })
}

#[tauri::command]
pub async fn confirm_pairing(
    state: State<'_, AppState>,
    device_id: String,
    code: String,
) -> Result<()> {
    state.pairing.lock().await.confirm(&device_id, &code)
}

#[tauri::command]
pub async fn get_trusted_devices(state: State<'_, AppState>) -> Result<Vec<TrustedDevice>> {
    Ok(state.trusted.lock().await.clone())
}

#[tauri::command]
pub async fn remove_trusted_device(state: State<'_, AppState>, device_id: String) -> Result<()> {
    let mut trusted = state.trusted.lock().await;
    let before = trusted.len();
    trusted.retain(|d| d.device_id != device_id);

    if trusted.len() == before {
        return Err(Error::DeviceNotFound(device_id));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

/// Starts sending files or folders to a discovered device (F05).
///
/// `paths` are filesystem paths the user chose through the OS picker — they are
/// local and trusted. Paths that arrive *from the peer* are a different matter
/// and never flow through this argument (SRS §91). A release build refuses
/// with `secureSessionRequired` until encrypted sessions land (D-F08-7).
#[tauri::command]
pub async fn start_transfer(
    state: State<'_, AppState>,
    device_id: String,
    paths: Vec<String>,
) -> Result<String> {
    state
        .orchestrator
        .clone()
        .start_send(device_id, paths)
        .await
}

fn table(state: &AppState) -> std::sync::MutexGuard<'_, HashMap<String, TransferProgress>> {
    match state.transfers.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    }
}

#[tauri::command]
pub async fn get_transfer(
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<TransferProgress> {
    table(&state)
        .get(&transfer_id)
        .cloned()
        .ok_or(Error::TransferNotFound(transfer_id))
}

#[tauri::command]
pub async fn list_transfers(state: State<'_, AppState>) -> Result<Vec<TransferProgress>> {
    Ok(table(&state).values().cloned().collect())
}

/// Cancels a transfer. Refused for one that already finished or was
/// cancelled (`TransferStatus::can_cancel`).
#[tauri::command]
pub async fn cancel_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<()> {
    state.orchestrator.stop(&transfer_id, StopReason::Cancel)
}

/// Pauses a moving transfer (`TransferStatus::can_pause`). The receiver keeps
/// its part files; the sender can resume.
#[tauri::command]
pub async fn pause_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<()> {
    state.orchestrator.stop(&transfer_id, StopReason::Pause)
}

/// Resumes a paused or interrupted send; only chunks the receiver is missing
/// are sent again (F06). Only the sending computer can resume.
#[tauri::command]
pub async fn resume_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<()> {
    state.orchestrator.resume(&transfer_id)
}

/// The receiving user accepts an incoming transfer (SRS §48).
#[tauri::command]
pub async fn accept_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<()> {
    state.orchestrator.answer(&transfer_id, Decision::Accept)
}

/// The receiving user declines an incoming transfer (SRS §48).
#[tauri::command]
pub async fn reject_transfer(state: State<'_, AppState>, transfer_id: String) -> Result<()> {
    state.orchestrator.answer(
        &transfer_id,
        Decision::Reject("Declined by the receiver.".into()),
    )
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationSettings {
    pub device_name: String,
    pub download_directory: String,
    pub conflict_resolution: crate::filesystem::ConflictResolution,
    pub verify_after_transfer: bool,
    pub resume_automatically: bool,
    pub preferred_transport: Option<String>,
    pub notifications_enabled: bool,
    pub theme: String,
    pub log_level: String,
}

impl Default for ApplicationSettings {
    fn default() -> Self {
        Self {
            device_name: hostname_fallback(),
            // SRS §45: `<Downloads>/FastTransfer` on every platform.
            download_directory: default_download_dir().display().to_string(),
            conflict_resolution: crate::filesystem::ConflictResolution::default(),
            verify_after_transfer: true,
            resume_automatically: true,
            preferred_transport: None,
            notifications_enabled: true,
            theme: "system".into(),
            log_level: "info".into(),
        }
    }
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<ApplicationSettings> {
    Ok(match state.settings.lock() {
        Ok(s) => s.clone(),
        Err(p) => p.into_inner().clone(),
    })
}

/// Updates settings for this run. Persisting them is Phase 9 (F10).
///
/// The download folder is validated here, not just at transfer time: it must
/// be absolute and must not be a system folder (SRS §49).
#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppState>,
    settings: ApplicationSettings,
) -> Result<()> {
    let destination = std::path::PathBuf::from(&settings.download_directory);
    if !destination.is_absolute() {
        return Err(Error::DestinationNotWritable {
            path: settings.download_directory.clone(),
            remedy: "Choose a full folder path, such as your Downloads folder.".into(),
        });
    }
    crate::filesystem::check_destination_root(&destination)?;

    match state.policy.lock() {
        Ok(mut p) => {
            p.destination = destination;
            p.conflict = settings.conflict_resolution;
        }
        Err(poisoned) => {
            let mut p = poisoned.into_inner();
            p.destination = destination;
            p.conflict = settings.conflict_resolution;
        }
    }
    match state.settings.lock() {
        Ok(mut s) => *s = settings,
        Err(p) => *p.into_inner() = settings,
    }
    Ok(())
}

/// Error payload type, exposed so the frontend's mirror can be checked against
/// it in tests.
#[tauri::command]
pub fn error_shape() -> ErrorPayload {
    Error::NoTransport.classify()
}

fn hostname_fallback() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "FastTransfer".to_string())
}
