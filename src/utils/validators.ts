/**
 * Runtime validation for data crossing the boundary.
 *
 * TypeScript types vanish at runtime. Anything arriving over IPC — and anything
 * a component receives from an event handler — is `unknown` until proven
 * otherwise. The Rust core validates its own inputs (see `filesystem.rs`); this
 * module is the frontend's matching discipline.
 */

import { isAppError, type AppError, type Peer, type TransferProgress } from "@/types";

/** Narrows an unknown value to a string, or returns null. */
export function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

/** Narrows an unknown value to a finite number, or returns null. */
export function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

/**
 * Validates a pairing code the user typed: six digits, optionally space- or
 * dash-separated. Returns the normalized `NNN NNN` form, or null.
 *
 * Normalizing here means the comparison in the Rust core sees exactly the shape
 * it issued.
 */
export function normalizePairingCode(input: string): string | null {
  const digits = input.replace(/[\s-]/g, "");
  if (!/^\d{6}$/.test(digits)) return null;
  return `${digits.slice(0, 3)} ${digits.slice(3)}`;
}

/** Whether a peer payload has the fields the UI needs to render a card. */
export function isPeer(value: unknown): value is Peer {
  if (typeof value !== "object" || value === null) return false;
  const p = value as Record<string, unknown>;
  return (
    typeof p.deviceId === "string" &&
    typeof p.displayName === "string" &&
    typeof p.link === "string" &&
    Array.isArray(p.addresses)
  );
}

/** Whether a progress payload has the fields the UI needs to render a bar. */
export function isTransferProgress(value: unknown): value is TransferProgress {
  if (typeof value !== "object" || value === null) return false;
  const p = value as Record<string, unknown>;
  return (
    typeof p.transferId === "string" &&
    typeof p.status === "string" &&
    typeof p.totalBytes === "number" &&
    typeof p.transferredBytes === "number"
  );
}

/**
 * A path the user picked locally is trusted; a path that arrived from a peer is
 * not. This is a naming aid for reviews, not a runtime check — the real
 * enforcement is `safe_join` in the Rust core (SRS §91).
 */
export type LocalPath = string & { readonly __brand: "LocalPath" };

/**
 * Validates a filename for display purposes only — e.g. warning the user that a
 * chosen save name is unusable. The authoritative check is the Rust side.
 */
export function fileNameIssue(name: string): string | null {
  if (!name.trim()) return "The name cannot be empty.";
  if (/[<>:"|?*]/.test(name)) {
    return "Windows does not allow < > : \" | ? * in a file name.";
  }
  if (/[. ]$/.test(name)) {
    return "A name cannot end with a space or a full stop.";
  }
  if (/^(CON|PRN|AUX|NUL|COM\d|LPT\d)$/i.test(name.split(".")[0] ?? "")) {
    return `"${name}" is a reserved name on Windows.`;
  }
  return null;
}

/** Narrows an unknown thrown value to an `AppError`, wrapping it if needed. */
export function asAppError(value: unknown): AppError {
  if (isAppError(value)) return value;
  return {
    kind: "internal",
    what: "Something went wrong.",
    why: typeof value === "string" ? value : null,
    remedy: "Retry.",
    resumable: false,
  };
}
