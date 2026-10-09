/**
 * Pairing and trust flows.
 *
 * Wraps the pairing commands so components deal in "start pairing", "confirm
 * the code the user typed" — not in device IDs and error tags. SRS §15, §33–§36.
 *
 * This module does not implement any cryptography. The handshake lives in the
 * Rust core and is Noise or TLS 1.3 from an established crate (SRS NFR-SEC-009).
 */

import * as api from "@/services/api";
import { logger } from "@/utils/logger";
import { normalizePairingCode } from "@/utils/validators";
import type { AppError, PairingChallenge, TrustedDevice } from "@/types";

export interface PairingSession {
  challenge: PairingChallenge;
  /** Seconds until the code stops working. Drives the countdown in the UI. */
  expiresAt: number;
}

/**
 * Begins pairing with a peer.
 *
 * The returned code must be shown on *both* screens for the user to compare.
 * A mismatch is the signal that something is between the two computers, which
 * is the entire point of the code (SRS FR-PAIR-003).
 */
export async function beginPairing(
  deviceId: string,
  displayName: string,
  platform: string,
): Promise<PairingSession> {
  logger.info("beginning pairing", { deviceId, platform });

  const challenge = await api.pairDevice(deviceId, displayName, platform);

  return {
    challenge,
    expiresAt: Date.now() + challenge.expiresInSeconds * 1000,
  };
}

/**
 * Submits the code the user reports seeing on the other computer.
 *
 * The code is normalized before sending so the Rust comparison sees exactly the
 * `NNN NNN` shape it issued. Returns the error rather than throwing so a wrong
 * code can be rendered inline without an error boundary catching it.
 */
export async function submitPairingCode(
  deviceId: string,
  rawCode: string,
): Promise<{ ok: true } | { ok: false; error: AppError }> {
  const code = normalizePairingCode(rawCode);

  if (!code) {
    return {
      ok: false,
      error: {
        kind: "authenticationFailed",
        what: "That is not a six-digit code.",
        why: null,
        remedy: "Check the code shown on the other computer and try again.",
        resumable: false,
      },
    };
  }

  try {
    await api.confirmPairing(deviceId, code);
    logger.info("pairing confirmed", { deviceId });
    return { ok: true };
  } catch (error) {
    const appError = error as AppError;
    logger.warn("pairing rejected", { deviceId, reason: appError.kind });
    return { ok: false, error: appError };
  }
}

export function isExpired(session: PairingSession): boolean {
  return Date.now() >= session.expiresAt;
}

export function secondsRemaining(session: PairingSession): number {
  return Math.max(0, Math.ceil((session.expiresAt - Date.now()) / 1000));
}

export const listTrustedDevices = (): Promise<TrustedDevice[]> => api.getTrustedDevices();

/**
 * Revokes trust. SRS FR-PAIR-008.
 *
 * There is no confirmation dialog here on purpose — that belongs to the caller,
 * which knows the device's display name and can ask a specific question rather
 * than a generic one.
 */
export async function revokeTrust(deviceId: string): Promise<void> {
  logger.info("revoking trust", { deviceId });
  await api.removeTrustedDevice(deviceId);
}
