import { describe, expect, it } from "vitest";

import type { TransferProgress, TransferStatus } from "@/types";
import { canCancel, canPause, canResume, upsertTransfer } from "@/utils/transferRules";

const ALL: TransferStatus[] = [
  "QUEUED",
  "PREPARING",
  "CONNECTING",
  "AUTHENTICATING",
  "TRANSFERRING",
  "VERIFYING",
  "COMPLETED",
  "PAUSED",
  "INTERRUPTED",
  "FAILED",
  "CANCELLED",
];

function progress(over: Partial<TransferProgress> = {}): TransferProgress {
  return {
    transferId: "t1",
    direction: "SEND",
    peerName: "Laptop B",
    status: "TRANSFERRING",
    totalBytes: 100,
    transferredBytes: 10,
    currentBps: 0,
    averageBps: 0,
    etaSeconds: null,
    currentFile: null,
    error: null,
    ...over,
  };
}

// These mirror `TransferStatus::can_pause`, `can_cancel` and `is_resumable` in
// transfer.rs. The core enforces them; the UI only hides buttons that would be
// refused (AC-F05-08).
describe("transfer button rules", () => {
  it("only a moving transfer can be paused", () => {
    const pausable = ALL.filter(canPause);
    expect(pausable).toEqual([
      "QUEUED",
      "PREPARING",
      "CONNECTING",
      "AUTHENTICATING",
      "TRANSFERRING",
      "VERIFYING",
    ]);
  });

  it("everything except completed and cancelled can be cancelled", () => {
    expect(ALL.filter((s) => !canCancel(s))).toEqual(["COMPLETED", "CANCELLED"]);
  });

  it("only the sending side can resume, and only from a stopped state", () => {
    for (const status of ["PAUSED", "INTERRUPTED", "FAILED"] as const) {
      expect(canResume(progress({ status }))).toBe(true);
      expect(canResume(progress({ status, direction: "RECEIVE" }))).toBe(false);
    }
    expect(canResume(progress({ status: "TRANSFERRING" }))).toBe(false);
    expect(canResume(progress({ status: "COMPLETED" }))).toBe(false);
  });

  it("no status is both pausable and resumable", () => {
    for (const status of ALL) {
      expect(canPause(status) && canResume(progress({ status }))).toBe(false);
    }
  });
});

describe("upsertTransfer", () => {
  it("appends a new transfer", () => {
    const list = upsertTransfer([], progress());
    expect(list).toHaveLength(1);
  });

  it("replaces an existing transfer in place, keeping order", () => {
    const a = progress({ transferId: "a" });
    const b = progress({ transferId: "b" });
    const updated = upsertTransfer([a, b], progress({ transferId: "a", transferredBytes: 99 }));
    expect(updated.map((p) => p.transferId)).toEqual(["a", "b"]);
    expect(updated[0]?.transferredBytes).toBe(99);
  });

  it("does not mutate the list it was given", () => {
    const original = [progress()];
    upsertTransfer(original, progress({ transferredBytes: 50 }));
    expect(original[0]?.transferredBytes).toBe(10);
  });
});
