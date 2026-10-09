import { useCallback, useEffect, useState } from "react";

import { PairingDialog } from "@/components/auth/PairingDialog";
import { DeviceCard } from "@/components/dashboard/DeviceCard";
import { LinkBanner } from "@/components/dashboard/LinkBanner";
import { ErrorNotice } from "@/components/shared/ErrorNotice";
import * as api from "@/services/api";
import * as auth from "@/services/auth";
import type { AppError, LinkInfo, PairingChallenge, Peer } from "@/types";
import { logger } from "@/utils/logger";

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

  const [pairing, setPairing] = useState<{
    challenge: PairingChallenge;
    expiresAt: number;
  } | null>(null);
  const [pairingBusy, setPairingBusy] = useState(false);
  const [pairingError, setPairingError] = useState<AppError | null>(null);
  const [secondsLeft, setSecondsLeft] = useState(0);

  // --- initial load and subscriptions --------------------------------------

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const [linkInfo, devices] = await Promise.all([
          api.getLinkInfo(),
          api.getDevices(),
        ]);
        if (cancelled) return;
        setLink(linkInfo);
        setPeers(devices);
      } catch (err) {
        if (!cancelled) setError(err as AppError);
      }
    }

    void load();

    // Every listener is torn down on unmount; leaking one would duplicate
    // device updates after the first re-render.
    const unlisteners: Array<() => void> = [];

    async function subscribe() {
      unlisteners.push(
        await api.on<LinkInfo>("link_changed", (next) => setLink(next)),
      );
      unlisteners.push(
        await api.on<Peer>("device_discovered", (peer) => {
          setPeers((current) =>
            current.some((p) => p.deviceId === peer.deviceId)
              ? current
              : [...current, peer],
          );
        }),
      );
      unlisteners.push(
        await api.on<Peer>("device_updated", (peer) => {
          setPeers((current) =>
            current.map((p) => (p.deviceId === peer.deviceId ? peer : p)),
          );
        }),
      );
      unlisteners.push(
        await api.on<{ deviceId: string }>("device_removed", ({ deviceId }) => {
          setPeers((current) => current.filter((p) => p.deviceId !== deviceId));
        }),
      );
    }

    void subscribe().catch((err) => logger.warn("event subscription failed", err));

    return () => {
      cancelled = true;
      for (const off of unlisteners) off();
    };
  }, []);

  // --- pairing countdown ---------------------------------------------------

  useEffect(() => {
    if (!pairing) return;

    const tick = () => {
      const remaining = Math.max(0, Math.ceil((pairing.expiresAt - Date.now()) / 1000));
      setSecondsLeft(remaining);
      if (remaining === 0) setPairing(null);
    };

    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [pairing]);

  // --- actions -------------------------------------------------------------

  const handleSend = useCallback(async (peer: Peer) => {
    setError(null);
    try {
      const session = await auth.beginPairing(peer.deviceId, peer.displayName, peer.platform);
      setPairingError(null);
      setPairing({ challenge: session.challenge, expiresAt: session.expiresAt });
    } catch (err) {
      setError(err as AppError);
    }
  }, []);

  const handleSubmitCode = useCallback(
    async (code: string) => {
      if (!pairing) return;
      setPairingBusy(true);
      setPairingError(null);

      const result = await auth.submitPairingCode(pairing.challenge.deviceId, code);

      setPairingBusy(false);
      if (result.ok) {
        setPairing(null);
        // TODO(Phase 4): open the file picker and start the transfer.
      } else {
        setPairingError(result.error);
      }
    },
    [pairing],
  );

  // --- render --------------------------------------------------------------

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center justify-between border-b border-border px-4 py-3">
        <h1 className="text-sm font-semibold">FastTransfer</h1>
      </header>

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
              computers, then connect a Thunderbolt cable or join the same network.
            </p>
          ) : (
            <div className="mt-3 space-y-2">
              {peers.map((peer) => (
                <DeviceCard
                  key={peer.deviceId}
                  peer={peer}
                  selected={selected?.deviceId === peer.deviceId}
                  onSelect={setSelected}
                  onSend={handleSend}
                />
              ))}
            </div>
          )}
        </section>

        <section className="mt-6 rounded-md border border-dashed border-border p-8 text-center">
          <p className="text-sm text-muted-foreground">Drop files here</p>
          <p className="my-2 text-xs text-muted-foreground">or</p>
          <p className="text-sm">Use Send Files on a device above</p>
        </section>
      </main>

      {pairing && (
        <PairingDialog
          challenge={pairing.challenge}
          secondsRemaining={secondsLeft}
          busy={pairingBusy}
          error={pairingError}
          onSubmit={handleSubmitCode}
          onCancel={() => {
            setPairing(null);
            setPairingError(null);
          }}
        />
      )}
    </div>
  );
}
