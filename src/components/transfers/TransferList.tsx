import { Button } from "@/components/shared/Button";
import type { TransferProgress } from "@/types";
import { formatBytes, formatDuration, formatSpeed, fraction } from "@/utils/helpers";
import { canCancel, canPause, canResume } from "@/utils/transferRules";

export interface TransferListProps {
  transfers: TransferProgress[];
  onPause: (id: string) => void;
  onResume: (id: string) => void;
  onCancel: (id: string) => void;
}

const STATUS_TEXT: Record<TransferProgress["status"], string> = {
  QUEUED: "Waiting",
  PREPARING: "Preparing",
  CONNECTING: "Connecting",
  AUTHENTICATING: "Connecting",
  TRANSFERRING: "Transferring",
  VERIFYING: "Verifying",
  COMPLETED: "Verified",
  PAUSED: "Paused",
  INTERRUPTED: "Interrupted",
  FAILED: "Failed",
  CANCELLED: "Cancelled",
};

/**
 * The transfers panel. FR-TRANSFER-007…009.
 *
 * Speed is labelled "measured" because it is (SRS §101); a completed transfer
 * says "Verified" only because the core reports `COMPLETED` after the SHA-256
 * check, never before (FR-RESUME-005).
 */
export function TransferList({ transfers, onPause, onResume, onCancel }: TransferListProps) {
  if (transfers.length === 0) return null;

  return (
    <ul className="mt-3 space-y-2" aria-label="Transfers">
      {transfers.map((t) => {
        const percent = Math.round(fraction(t.transferredBytes, t.totalBytes) * 100);
        const moving = t.status === "TRANSFERRING";
        return (
          <li key={t.transferId} className="rounded-md border border-border p-3">
            <div className="flex items-center justify-between gap-3">
              <p className="min-w-0 truncate text-sm font-medium">
                {t.direction === "SEND" ? "To " : "From "}
                {t.peerName || "another computer"}
              </p>
              <span className="text-xs text-muted-foreground">{STATUS_TEXT[t.status]}</span>
            </div>

            {t.currentFile && moving && (
              <p className="mt-0.5 truncate text-xs text-muted-foreground">{t.currentFile}</p>
            )}

            <div
              className="mt-2 h-1.5 w-full overflow-hidden rounded bg-muted"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={percent}
              aria-label="Transfer progress"
            >
              <div className="h-full bg-primary" style={{ width: `${percent}%` }} />
            </div>

            <p className="mt-1 text-xs text-muted-foreground">
              {formatBytes(t.transferredBytes)} / {formatBytes(t.totalBytes)}
              {moving && t.currentBps > 0 && <> · {formatSpeed(t.currentBps)} (measured)</>}
              {moving && t.etaSeconds !== null && <> · {formatDuration(t.etaSeconds)} left</>}
            </p>

            {t.error && (
              <p role="alert" className="mt-1 text-xs text-destructive">
                {t.error.what}
                {t.error.remedy ? ` ${t.error.remedy}` : ""}
              </p>
            )}

            <div className="mt-2 flex justify-end gap-2">
              {canPause(t.status) && (
                <Button size="sm" variant="ghost" onClick={() => onPause(t.transferId)}>
                  Pause
                </Button>
              )}
              {canResume(t) && (
                <Button size="sm" variant="secondary" onClick={() => onResume(t.transferId)}>
                  Resume
                </Button>
              )}
              {canCancel(t.status) && (
                <Button size="sm" variant="ghost" onClick={() => onCancel(t.transferId)}>
                  Cancel
                </Button>
              )}
            </div>
          </li>
        );
      })}
    </ul>
  );
}
