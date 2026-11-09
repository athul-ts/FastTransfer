import { Button } from "@/components/shared/Button";
import type { Peer, TransportKind } from "@/types";
import { cx, linkLabel } from "@/utils/helpers";

const TRANSPORT_LABEL: Record<TransportKind, string> = {
  thunderbolt: "Thunderbolt",
  ethernet: "Ethernet",
  wifi: "Wi-Fi",
};

export interface DeviceCardProps {
  peer: Peer;
  selected: boolean;
  onSelect: (peer: Peer) => void;
  onSend: (peer: Peer) => void;
}

/**
 * One peer in the nearby-devices list. SRS §37.
 *
 * The card shows the trust state explicitly: a device found on the cable is a
 * stranger until the user approves it (SRS NFR-SEC-002). "Send Files" opens the
 * picker; it grants nothing on its own, because the receiving user approves
 * every transfer (D-F05-1). Pairing arrives with F08.
 */
export function DeviceCard({ peer, selected, onSelect, onSend }: DeviceCardProps) {
  const overCable = peer.link === "THUNDERBOLT" || peer.link === "USB4_NETWORK";
  // The path this computer has to the peer, judged locally (D-F03-4).
  const path = peer.transport ? TRANSPORT_LABEL[peer.transport] : linkLabel(peer.link);

  return (
    <div
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      onClick={() => onSelect(peer)}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect(peer);
        }
      }}
      className={cx(
        "cursor-pointer rounded-md border p-3 transition-colors",
        "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary",
        selected ? "border-primary bg-muted" : "border-border hover:bg-muted/50",
      )}
    >
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0">
          <p className="truncate font-medium">
            {overCable && <span aria-hidden="true">⚡ </span>}
            {peer.displayName}
          </p>

          <p className="mt-0.5 text-xs text-muted-foreground">
            {peer.platform} · {path}
          </p>

          <p className="mt-1 text-xs">
            {peer.trusted ? (
              <span className="text-foreground">Trusted</span>
            ) : (
              <span className="text-muted-foreground">Not yet approved</span>
            )}
          </p>
        </div>

        <Button
          size="sm"
          variant={selected ? "primary" : "secondary"}
          onClick={(event) => {
            // Without this the click also selects the card, which re-renders
            // and can swallow the send action.
            event.stopPropagation();
            onSend(peer);
          }}
        >
          Send Files
        </Button>
      </div>
    </div>
  );
}
