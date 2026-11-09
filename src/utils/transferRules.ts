/**
 * Which transfer controls make sense for a given state.
 *
 * These mirror `TransferStatus::can_pause`, `can_cancel` and `is_resumable` in
 * `src-tauri/src/transfer.rs`. The core enforces the rules and refuses anything
 * else with `invalidTransferState`; the UI uses these only to avoid offering a
 * button that would be refused (AC-F05-08).
 */

import type { TransferProgress, TransferStatus } from "@/types";

const TERMINAL: ReadonlySet<TransferStatus> = new Set(["COMPLETED", "FAILED", "CANCELLED"]);
const RESUMABLE: ReadonlySet<TransferStatus> = new Set(["INTERRUPTED", "PAUSED", "FAILED"]);

/** Anything still moving can be paused. */
export function canPause(status: TransferStatus): boolean {
  return !TERMINAL.has(status) && !RESUMABLE.has(status);
}

/** Everything except a finished or already-cancelled transfer can be cancelled. */
export function canCancel(status: TransferStatus): boolean {
  return status !== "COMPLETED" && status !== "CANCELLED";
}

/**
 * Only the sending computer can resume: the receiver keeps its part files and
 * waits for the sender to reconnect (D-F06-4).
 */
export function canResume(progress: TransferProgress): boolean {
  return progress.direction === "SEND" && RESUMABLE.has(progress.status);
}

/** Returns a new list with `next` replacing the entry with the same id, or appended. */
export function upsertTransfer(
  list: readonly TransferProgress[],
  next: TransferProgress,
): TransferProgress[] {
  const index = list.findIndex((p) => p.transferId === next.transferId);
  if (index === -1) return [...list, next];
  const copy = [...list];
  copy[index] = next;
  return copy;
}
