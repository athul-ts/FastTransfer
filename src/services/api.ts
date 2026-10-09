/**
 * The only place the frontend talks to the Rust core.
 *
 * Components import from here and never call `invoke` directly. That keeps the
 * command names, argument shapes, and error handling in one file — so when a
 * Rust signature changes, there is exactly one place to fix (see CLAUDE.md).
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import {
  isAppError,
  type AppError,
  type ApplicationSettings,
  type Diagnostics,
  type EventName,
  type LinkInfo,
  type PairingChallenge,
  type Peer,
  type TransferProgress,
  type TransportCandidate,
  type TrustedDevice,
} from "@/types";
import { logger } from "@/utils/logger";

/**
 * Normalizes whatever the backend threw into an `AppError`.
 *
 * Rust returns the structured payload, but a panic in the core surfaces as a
 * plain string. Rather than let that reach the UI raw, it is wrapped — the user
 * must never see an internal error with no explanation.
 */
function toAppError(error: unknown): AppError {
  if (isAppError(error)) return error;

  logger.error("non-structured error from core", error);

  return {
    kind: "internal",
    what: "Something went wrong inside FastTransfer.",
    why: typeof error === "string" ? error : null,
    remedy: "Retry. If it keeps happening, check the logs in Settings > Advanced.",
    resumable: false,
  };
}

async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

// ---------------------------------------------------------------------------
// Link and transport
// ---------------------------------------------------------------------------

export const getLinkInfo = (): Promise<LinkInfo> => call("get_link_info");

export const getTransports = (): Promise<TransportCandidate[]> => call("get_transports");

export const getDevices = (): Promise<Peer[]> => call("get_devices");

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

export const getDiagnostics = (): Promise<Diagnostics> => call("get_diagnostics");

// ---------------------------------------------------------------------------
// Pairing and trust
// ---------------------------------------------------------------------------

export const pairDevice = (
  deviceId: string,
  displayName: string,
  platform: string,
): Promise<PairingChallenge> =>
  call("pair_device", { deviceId, displayName, platform });

export const confirmPairing = (deviceId: string, code: string): Promise<void> =>
  call("confirm_pairing", { deviceId, code });

export const getTrustedDevices = (): Promise<TrustedDevice[]> => call("get_trusted_devices");

export const removeTrustedDevice = (deviceId: string): Promise<void> =>
  call("remove_trusted_device", { deviceId });

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

export const startTransfer = (deviceId: string, paths: string[]): Promise<string> =>
  call("start_transfer", { deviceId, paths });

export const getTransfer = (transferId: string): Promise<TransferProgress> =>
  call("get_transfer", { transferId });

export const listTransfers = (): Promise<TransferProgress[]> => call("list_transfers");

export const cancelTransfer = (transferId: string): Promise<void> =>
  call("cancel_transfer", { transferId });

export const pauseTransfer = (transferId: string): Promise<void> =>
  call("pause_transfer", { transferId });

export const resumeTransfer = (transferId: string): Promise<void> =>
  call("resume_transfer", { transferId });

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

export const getSettings = (): Promise<ApplicationSettings> => call("get_settings");

export const updateSettings = (settings: ApplicationSettings): Promise<void> =>
  call("update_settings", { settings });

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/**
 * Subscribes to a core event.
 *
 * Returns the unlisten function directly so callers can pass it straight to a
 * React effect's cleanup. Forgetting to unsubscribe leaks a listener per mount,
 * which shows up as duplicated progress updates after a few navigations.
 */
export function on<T>(event: EventName, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}

/**
 * Subscribes and logs a warning instead of throwing if the core is unavailable.
 * Used for non-critical streams like diagnostics.
 */
export async function onOptional<T>(
  event: EventName,
  handler: (payload: T) => void,
): Promise<UnlistenFn | null> {
  try {
    return await on<T>(event, handler);
  } catch (error) {
    logger.warn(`could not subscribe to ${event}`, error);
    return null;
  }
}

export type { AppError };
