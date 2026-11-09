//! FastTransfer core.
//!
//! A cable-first direct file transfer engine. The React frontend reaches this
//! crate only through the commands in [`commands`]; everything performance- or
//! security-critical lives here (SRS §11.1).
//!
//! # Module map
//!
//! | Module | Owns | SRS |
//! |---|---|---|
//! | [`link`] | what a plugged cable can actually do | §4, §17 |
//! | [`discovery`] | finding peers, including with no router | §13 |
//! | [`transport`] | interface enumeration, ranking, fallback | §14, §16 |
//! | [`protocol`] | framing, messages, negotiation | §18, §19, §58, §59 |
//! | [`transfer`] | chunking, state machine, resume, backpressure | §20–§22, §27–§29 |
//! | [`security`] | identity, pairing, trust | §12, §15, §34–§36 |
//! | [`filesystem`] | I/O and the path-safety boundary | §23–§26, §91 |
//! | [`error`] | errors mapped to user-facing meaning | §95 |

pub mod commands;
pub mod discovery;
pub mod error;
pub mod filesystem;
pub mod link;
pub mod orchestrator;
pub mod protocol;
pub mod security;
pub mod session;
pub mod transfer;
pub mod transport;

use std::sync::Arc;

use tauri::{Emitter, Manager};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

use crate::commands::AppState;
use crate::discovery::{Advertisement, Discovery, DiscoveryEvent, DEFAULT_PORT};
use crate::security::IdentityStore;

/// Initializes the tracing subscriber.
///
/// Defaults to `info` in release and `debug` in development. `RUST_LOG` or
/// `FASTTRANSFER_LOG` overrides it. Level `trace` still must not log file
/// contents or key material — the filter is not a licence (SRS NFR-SEC-007).
fn init_tracing() {
    let default = if cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    let filter = EnvFilter::try_from_env("FASTTRANSFER_LOG")
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new(format!("fasttransfer_lib={default},warn")));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true))
        .with(filter)
        .init();
}

/// Builds and runs the Tauri application.
pub fn run() {
    init_tracing();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_os::init())
        .setup(|app| {
            let identity_store = IdentityStore::open()?;
            let identity = identity_store.load_or_create(
                std::env::var("COMPUTERNAME")
                    .or_else(|_| std::env::var("HOSTNAME"))
                    .unwrap_or_else(|_| "FastTransfer".to_string()),
            )?;

            let advertisement = Advertisement::new(
                identity.device_id.clone(),
                identity.display_name.clone(),
                std::env::consts::OS.to_string(),
                DEFAULT_PORT,
            );

            let discovery = Arc::new(Discovery::new(advertisement));

            // Peer changes go straight to the window (FR-DISC-008, D-F03-5).
            let handle = app.handle().clone();
            discovery.set_event_sink(Arc::new(move |event| {
                let result = match &event {
                    DiscoveryEvent::Discovered(peer) => handle.emit("device_discovered", peer),
                    DiscoveryEvent::Updated(peer) => handle.emit("device_updated", peer),
                    DiscoveryEvent::Removed { device_id } => handle.emit(
                        "device_removed",
                        serde_json::json!({ "deviceId": device_id }),
                    ),
                };
                if let Err(err) = result {
                    tracing::debug!(%err, "could not emit a discovery event");
                }
            }));

            let state = AppState::new(app.handle().clone(), discovery.clone(), identity);
            let orchestrator = state.orchestrator.clone();
            app.manage(state);

            // Discovery and the listener run in the background so the window
            // opens immediately rather than blocking on the network (SRS §69).
            // The clone shares the peer table with `AppState`, so peers the
            // background task finds are visible to `get_devices`.
            tauri::async_runtime::spawn(async move {
                if let Err(err) = discovery.start().await {
                    tracing::error!(%err, "discovery failed to start");
                }
            });
            tauri::async_runtime::spawn(async move {
                if let Err(err) = orchestrator.listen(DEFAULT_PORT).await {
                    tracing::error!(%err, "transfer listener failed to start");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Link and transport
            commands::get_link_info,
            commands::get_transports,
            commands::get_devices,
            // Diagnostics
            commands::get_diagnostics,
            // Lets the frontend assert its error mirror against the Rust model
            // rather than against a hand-copied snapshot.
            commands::error_shape,
            // Pairing and trust
            commands::pair_device,
            commands::confirm_pairing,
            commands::get_trusted_devices,
            commands::remove_trusted_device,
            // Transfers
            commands::start_transfer,
            commands::get_transfer,
            commands::list_transfers,
            commands::cancel_transfer,
            commands::pause_transfer,
            commands::resume_transfer,
            commands::accept_transfer,
            commands::reject_transfer,
            // Settings
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running FastTransfer");
}
