/**
 * Local persistence for transfer history and settings.
 *
 * Everything here stays on this machine. No cloud, no account, no telemetry
 * (SRS §71). History holds **metadata only** — never file contents — and file
 * names are confined to this device (SRS §44, §69).
 *
 * Storage is `localStorage`, accessed defensively: it can throw in a private
 * window and can come back empty. A failure here degrades the History screen;
 * it must never break a transfer.
 */

import { logger } from "@/utils/logger";
import type { ApplicationSettings, TransferStatus } from "@/types";

const HISTORY_KEY = "fasttransfer.history.v1";
const SETTINGS_KEY = "fasttransfer.settings.v1";

/** A completed or failed transfer, as kept in local history. */
export interface HistoryEntry {
  transferId: string;
  /** File or folder name. Metadata only — the contents are never stored. */
  name: string;
  peerDisplayName: string;
  totalBytes: number;
  status: TransferStatus;
  /** Unix seconds. */
  finishedAt: number;
  averageBps: number;
  linkLabel: string;
}

const MAX_HISTORY_ENTRIES = 500;

function safeRead<T>(key: string, fallback: T): T {
  try {
    const raw = window.localStorage.getItem(key);
    if (!raw) return fallback;
    return JSON.parse(raw) as T;
  } catch (error) {
    // A corrupt entry must not brick the app — fall back and move on.
    logger.warn(`could not read ${key}; using defaults`, error);
    return fallback;
  }
}

function safeWrite(key: string, value: unknown): boolean {
  try {
    window.localStorage.setItem(key, JSON.stringify(value));
    return true;
  } catch (error) {
    // Quota exceeded, or storage blocked. Non-fatal by design.
    logger.warn(`could not write ${key}`, error);
    return false;
  }
}

// ---------------------------------------------------------------------------
// History
// ---------------------------------------------------------------------------

export function loadHistory(): HistoryEntry[] {
  const entries = safeRead<HistoryEntry[]>(HISTORY_KEY, []);
  return Array.isArray(entries) ? entries : [];
}

/**
 * Appends a history entry, newest first, trimmed to the cap.
 *
 * Failed and interrupted transfers are recorded too — the History screen shows
 * an interrupted transfer explicitly (SRS §46), so dropping them here would
 * hide the thing the user most needs to see.
 */
export function recordTransfer(entry: HistoryEntry): void {
  const entries = [entry, ...loadHistory()].slice(0, MAX_HISTORY_ENTRIES);
  safeWrite(HISTORY_KEY, entries);
}

export function clearHistory(): void {
  try {
    window.localStorage.removeItem(HISTORY_KEY);
  } catch (error) {
    logger.warn("could not clear history", error);
  }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

export const DEFAULT_SETTINGS: ApplicationSettings = {
  deviceName: "",
  downloadDirectory: "",
  // Never destructive by default (SRS §24).
  conflictResolution: "keepBoth",
  verifyAfterTransfer: true,
  resumeAutomatically: true,
  preferredTransport: null,
  notificationsEnabled: true,
  theme: "system",
  logLevel: "info",
};

/**
 * Loads settings, filling in any field a previous version did not have.
 *
 * Merging over the defaults rather than trusting the stored object means an
 * upgrade that adds a setting does not produce `undefined` in the UI.
 */
export function loadSettings(): ApplicationSettings {
  const stored = safeRead<Partial<ApplicationSettings>>(SETTINGS_KEY, {});
  return { ...DEFAULT_SETTINGS, ...stored };
}

export function saveSettings(settings: ApplicationSettings): boolean {
  return safeWrite(SETTINGS_KEY, settings);
}
