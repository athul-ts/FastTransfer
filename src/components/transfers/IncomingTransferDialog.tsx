import { Button } from "@/components/shared/Button";
import type { IncomingTransfer } from "@/types";
import { formatBytes } from "@/utils/helpers";

export interface IncomingTransferDialogProps {
  request: IncomingTransfer;
  busy: boolean;
  onAccept: () => void;
  onReject: () => void;
}

/**
 * Asks the receiving user to approve an incoming transfer. SRS §48, D-F05-1.
 *
 * Until encrypted pairing exists (F08) this dialog is the only gate between a
 * computer on the link and the download folder, so it shows everything the
 * user needs to decide: who, what, how much, and where it will go. The sender's
 * name is self-declared and is labelled as such — it proves nothing.
 */
export function IncomingTransferDialog({
  request,
  busy,
  onAccept,
  onReject,
}: IncomingTransferDialogProps) {
  const extra = request.fileCount - request.topLevel.length;

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="incoming-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
    >
      <div className="w-full max-w-md rounded-lg border border-border bg-background p-5 shadow-lg">
        <h2 id="incoming-title" className="text-base font-semibold">
          Incoming transfer
        </h2>

        <dl className="mt-3 grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 text-sm">
          <dt className="text-muted-foreground">From</dt>
          <dd className="truncate">
            {request.peerName}{" "}
            <span className="text-muted-foreground">({request.peerPlatform}, name not verified)</span>
          </dd>

          <dt className="text-muted-foreground">Files</dt>
          <dd>
            {request.topLevel.join(", ")}
            {extra > 0 && <span className="text-muted-foreground"> and {extra} more</span>}
          </dd>

          <dt className="text-muted-foreground">Total</dt>
          <dd>{formatBytes(request.totalBytes)}</dd>

          <dt className="text-muted-foreground">Destination</dt>
          <dd className="break-all">{request.destination}</dd>
        </dl>

        <p className="mt-3 text-xs text-muted-foreground">
          Accept only if you expected this transfer.
        </p>

        <div className="mt-4 flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={onReject} disabled={busy}>
            Reject
          </Button>
          <Button type="button" variant="primary" onClick={onAccept} loading={busy}>
            Accept
          </Button>
        </div>
      </div>
    </div>
  );
}
