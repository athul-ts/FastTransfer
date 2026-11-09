import { useCallback, useEffect, useRef, useState } from "react";

import { DeviceCard } from "@/components/dashboard/DeviceCard";
import { LinkBanner } from "@/components/dashboard/LinkBanner";
import { Button } from "@/components/shared/Button";
import { ErrorNotice } from "@/components/shared/ErrorNotice";
import { PreviewLabel } from "@/components/shared/PreviewLabel";
import { IncomingTransferDialog } from "@/components/transfers/IncomingTransferDialog";
import { TransferList } from "@/components/transfers/TransferList";
import * as api from "@/services/api";
import { onFilesDropped, pickFiles, pickFolder } from "@/services/files";
import type { AppError, EventName, IncomingTransfer, LinkInfo, Peer, TransferProgress } from "@/types";
import { logger } from "@/utils/logger";
import { upsertTransfer } from "@/utils/transferRules";
import { isTransferProgress } from "@/utils/validators";

/** Every core event that carries a `TransferProgress`. */
const TRANSFER_EVENTS: EventName[] = [
  "transfer_started",
  "transfer_progress",
  "transfer_paused",
  "transfer_resumed",
  "transfer_completed",
  "transfer_failed",
  "transfer_cancelled",
];

/**
 * Application shell.
 *
 * State here is **presentation state only**. The Rust core is the source of
 * truth for transfers, peers, and link status (SRS §80) — this component holds
 * a cache of what the core last told it, refreshed by events.
 */
export function App() {
  const [link, setLink] = useState<LinkInfo | null>(null);
  const [peers, setPeers] = useState<Peer[]>([]);
  const [selected, setSelected] = useState<Peer | null>(null);
  const [error, setError] = useState<AppError | null>(null);
  const [transfers, setTransfers] = useState<TransferProgress[]>([]);
  const [incoming, setIncoming] = useState<IncomingTransfer[]>([]);
  const [answering, setAnswering] = useState(false);

  // The drop handler is registered once; these let it see the current values.
  const selectedRef = useRef<Peer | null>(null);
  const peersRef = useRef<Peer[]>([]);
  selectedRef.current = selected;
  peersRef.current = peers;

  // --- initial load and subscriptions --------------------------------------

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const [linkInfo, devices, existing] = await Promise.all([
          api.getLinkInfo(),
          api.getDevices(),
          api.listTransfers(),
        ]);
        if (cancelled) return;
        setLink(linkInfo);
        setPeers(devices);
        setTransfers(existing);
      } catch (err) {
        if (!cancelled) setError(err as AppError);
      }
    }

    void load();

    // Every listener is torn down on unmount; leaking one would duplicate
    // device updates after the first re-render.
    const unlisteners: Array<() => void> = [];

    async function subscribe() {
      unlisteners.push(await api.on<LinkInfo>("link_changed", (next) => setLink(next)));
      unlisteners.push(
        await api.on<Peer>("device_discovered", (peer) => {
          setPeers((current) =>
            current.some((p) => p.deviceId === peer.deviceId) ? current : [...current, peer],
          );
        }),
      );
      unlisteners.push(
        await api.on<Peer>("device_updated", (peer) => {
          setPeers((current) => current.map((p) => (p.deviceId === peer.deviceId ? peer : p)));
        }),
      );
      unlisteners.push(
        await api.on<{ deviceId: string }>("device_removed", ({ deviceId }) => {
          setPeers((current) => current.filter((p) => p.deviceId !== deviceId));
        }),
      );
      for (const name of TRANSFER_EVENTS) {
        unlisteners.push(
          await api.on<unknown>(name, (payload) => {
            if (!isTransferProgress(payload)) return;
            setTransfers((current) => upsertTransfer(current, payload));
            // A request the receiver has not answered disappears from the
            // dialog queue once the core moves it on (timed out, withdrawn).
            if (payload.status !== "QUEUED") {
              setIncoming((q) => q.filter((r) => r.transferId !== payload.transferId));
            }
          }),
        );
      }
      unlisteners.push(
        await api.on<IncomingTransfer>("incoming_transfer", (request) => {
          setIncoming((q) =>
            q.some((r) => r.transferId === request.transferId) ? q : [...q, request],
          );
        }),
      );
      unlisteners.push(
        await onFilesDropped((paths) => {
          // Refs, not state updaters: an updater may run twice and must never
          // start a transfer (D-F05-6).
          const target =
            selectedRef.current ?? (peersRef.current.length === 1 ? peersRef.current[0] : null);
          if (target) {
            sendTo(target, paths).catch((err) => setError(err as AppError));
          } else {
            setError({
              kind: "peerNotFound",
              what: "Choose a computer first.",
              why: "Files were dropped, but no nearby computer is selected.",
              remedy: "Select a device in the list, then drop the files again.",
              resumable: false,
            });
          }
        }),
      );
    }

    void subscribe().catch((err) => logger.warn("event subscription failed", err));

    return () => {
      cancelled = true;
      for (const off of unlisteners) off();
    };
  }, []);

  // --- actions -------------------------------------------------------------

  // The receiver approves each transfer (D-F05-1); pairing arrives with F08,
  // so the send path goes straight to the picker (D-F05-3).
  const handleSendFiles = useCallback(async (peer: Peer) => {
    setError(null);
    try {
      await sendTo(peer, await pickFiles());
    } catch (err) {
      setError(err as AppError);
    }
  }, []);

  const handleSendFolder = useCallback(async (peer: Peer) => {
    setError(null);
    try {
      await sendTo(peer, await pickFolder());
    } catch (err) {
      setError(err as AppError);
    }
  }, []);

  const control = useCallback(
    (action: (id: string) => Promise<void>) => (id: string) => {
      action(id).catch((err) => setError(err as AppError));
    },
    [],
  );

  const answer = useCallback(async (request: IncomingTransfer, accept: boolean) => {
    setAnswering(true);
    try {
      await (accept ? api.acceptTransfer(request.transferId) : api.rejectTransfer(request.transferId));
    } catch (err) {
      setError(err as AppError);
    } finally {
      setAnswering(false);
      setIncoming((q) => q.filter((r) => r.transferId !== request.transferId));
    }
  }, []);

  const pending = incoming[0];

  // --- render --------------------------------------------------------------

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-border px-4 py-3">
        <h1 className="text-sm font-semibold">FastTransfer</h1>
      </header>

      <PreviewLabel />
      <LinkBanner link={link} />

      <main className="flex-1 overflow-auto p-4">
        {error && (
          <div className="mb-4">
            <ErrorNotice error={error} onDismiss={() => setError(null)} />
          </div>
        )}

        <section aria-labelledby="nearby-heading">
          <h2 id="nearby-heading" className="text-xs font-medium uppercase text-muted-foreground">
            Nearby Devices
          </h2>

          {peers.length === 0 ? (
            <p className="mt-3 text-sm text-muted-foreground">
              No other computer found yet. Check that FastTransfer is open on both
              computers, then connect a Thunderbolt or Ethernet cable, or join the same network.
            </p>
          ) : (
            <div className="mt-3 space-y-2">
              {peers.map((peer) => (
                <DeviceCard
                  key={peer.deviceId}
                  peer={peer}
                  selected={selected?.deviceId === peer.deviceId}
                  onSelect={setSelected}
                  onSend={(p) => void handleSendFiles(p)}
                />
              ))}
            </div>
          )}
        </section>

        <section className="mt-6 rounded-md border border-dashed border-border p-8 text-center">
          <p className="text-sm text-muted-foreground">Drop files or folders here</p>
          <p className="my-2 text-xs text-muted-foreground">or</p>
          <div className="flex justify-center gap-2">
            <Button
              variant="secondary"
              disabled={!selected}
              onClick={() => selected && void handleSendFiles(selected)}
            >
              Select Files
            </Button>
            <Button
              variant="secondary"
              disabled={!selected}
              onClick={() => selected && void handleSendFolder(selected)}
            >
              Select Folder
            </Button>
          </div>
          {!selected && peers.length > 0 && (
            <p className="mt-2 text-xs text-muted-foreground">Select a device above first.</p>
          )}
        </section>

        <section aria-labelledby="transfers-heading" className="mt-6">
          {transfers.length > 0 && (
            <h2 id="transfers-heading" className="text-xs font-medium uppercase text-muted-foreground">
              Transfers
            </h2>
          )}
          <TransferList
            transfers={transfers}
            onPause={control(api.pauseTransfer)}
            onResume={control(api.resumeTransfer)}
            onCancel={control(api.cancelTransfer)}
          />
        </section>
      </main>

      {pending && (
        <IncomingTransferDialog
          request={pending}
          busy={answering}
          onAccept={() => void answer(pending, true)}
          onReject={() => void answer(pending, false)}
        />
      )}
    </div>
  );
}

async function sendTo(peer: Peer, paths: string[]): Promise<void> {
  if (paths.length === 0) return; // the user cancelled the picker
  logger.info("starting transfer", { deviceId: peer.deviceId, items: paths.length });
  await api.startTransfer(peer.deviceId, paths);
}
