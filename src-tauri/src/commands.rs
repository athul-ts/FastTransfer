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

use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, State};
use tokio::sync::Mutex;

use crate::discovery::{Discovery, Peer};
use crate::error::{Error, ErrorPayload, Result};
use crate::link::{self, LinkExplanation, LinkKind};
use crate::security::{DeviceIdentity, PairingState, TrustedDevice};
use crate::transfer::{TransferProgress, TransferStatus};
use crate::transport::{self, TransportCandidate};

/// Application state shared across commands.
pub struct AppState {
    pub discovery: Arc<Discovery>,
    pub identity: DeviceIdentity,
    pub pairing: Arc<Mutex<PairingState>>,
    pub trusted: Arc<Mutex<Vec<TrustedDevice>>>,
    pub transfers: Arc<Mutex<std::collections::HashMap<String, TransferProgress>>>,
}

impl AppState {
    pub fn new(discovery: Discovery, identity: DeviceIdentity) -> Self {
        Self {
            discovery: Arc::new(discovery),
            identity,
            pairing: Arc::new(Mutex::new(PairingState::new())),
            trusted: Arc::new(Mutex::new(Vec::new())),
            transfers: Arc::new(Mutex::new(std::collections::HashMap::new())),
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

#[tauri::command]
pub fn get_link_info() -> LinkInfo {
    let (kind, explanation) = link::current();
    LinkInfo {
        kind,
        explanation,
        measured_bps: None,
    }
}

#[tauri::command]
pub fn get_transports() -> Vec<TransportCandidate> {
    transport::candidates()
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
    let (kind, explanation) = link::current();
    let transports = transport::candidates();
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
        interfaces: transport::enumerate_interfaces(),
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
        return Err(Error::TransferNotFound(device_id));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

/// Starts a transfer.
///
/// `paths` are filesystem paths the user chose through the OS picker — they are
/// local and trusted. Paths that arrive *from the peer* are a different matter
/// and never flow through this argument (SRS §91).
///
/// TODO(Phase 4, SRS §108): build the manifest, connect, and run the engine.
#[tauri::command]
pub async fn start_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    device_id: String,
    paths: Vec<String>,
) -> Result<String> {
    let transfer_id = uuid::Uuid::new_v4().to_string();

    let progress = TransferProgress {
        transfer_id: transfer_id.clone(),
        status: TransferStatus::Queued,
        total_bytes: 0,
        transferred_bytes: 0,
        current_bps: 0,
        average_bps: 0,
        eta_seconds: None,
        current_file: None,
    };

    state
        .transfers
        .lock()
        .await
        .insert(transfer_id.clone(), progress.clone());

    // The UI must see the queued state immediately, before any I/O happens.
    let _ = app.emit("transfer_started", &progress);
    let _ = (device_id, paths);

    Ok(transfer_id)
}

#[tauri::command]
pub async fn get_transfer(
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<TransferProgress> {
    state
        .transfers
        .lock()
        .await
        .get(&transfer_id)
        .cloned()
        .ok_or(Error::TransferNotFound(transfer_id))
}

#[tauri::command]
pub async fn list_transfers(state: State<'_, AppState>) -> Result<Vec<TransferProgress>> {
    Ok(state.transfers.lock().await.values().cloned().collect())
}

#[tauri::command]
pub async fn cancel_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<()> {
    let mut transfers = state.transfers.lock().await;
    let progress = transfers
        .get_mut(&transfer_id)
        .ok_or_else(|| Error::TransferNotFound(transfer_id.clone()))?;

    progress.status = TransferStatus::Cancelled;
    let snapshot = progress.clone();
    drop(transfers);

    let _ = app.emit("transfer_cancelled", &snapshot);
    Ok(())
}

#[tauri::command]
pub async fn pause_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<()> {
    let mut transfers = state.transfers.lock().await;
    let progress = transfers
        .get_mut(&transfer_id)
        .ok_or_else(|| Error::TransferNotFound(transfer_id.clone()))?;

    progress.status = TransferStatus::Paused;
    let snapshot = progress.clone();
    drop(transfers);

    let _ = app.emit("transfer_paused", &snapshot);
    Ok(())
}

/// Resumes an interrupted transfer.
///
/// Only transfers in a resumable state may be resumed — resuming a `Completed`
/// or `Queued` transfer is a programming error on the caller's side, so it is
/// rejected rather than silently accepted.
#[tauri::command]
pub async fn resume_transfer(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    transfer_id: String,
) -> Result<()> {
    let mut transfers = state.transfers.lock().await;
    let progress = transfers
        .get_mut(&transfer_id)
        .ok_or_else(|| Error::TransferNotFound(transfer_id.clone()))?;

    if !progress.status.is_resumable() {
        return Err(Error::MalformedMessage(format!(
            "transfer {transfer_id} is {:?} and cannot be resumed",
            progress.status
        )));
    }

    progress.status = TransferStatus::Transferring;
    let snapshot = progress.clone();
    drop(transfers);

    let _ = app.emit("transfer_resumed", &snapshot);
    Ok(())
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
            download_directory: String::new(),
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
pub async fn get_settings() -> Result<ApplicationSettings> {
    // TODO(Phase 9): load from the app-config directory.
    Ok(ApplicationSettings::default())
}

#[tauri::command]
pub async fn update_settings(settings: ApplicationSettings) -> Result<()> {
    // TODO(Phase 9): validate and persist.
    let _ = settings;
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
