import { useEffect, useState } from "react";

import { Button } from "@/components/shared/Button";
import { ErrorNotice } from "@/components/shared/ErrorNotice";
import type { AppError, PairingChallenge } from "@/types";
import { cx } from "@/utils/helpers";

export interface PairingDialogProps {
  challenge: PairingChallenge;
  secondsRemaining: number;
  busy: boolean;
  error: AppError | null;
  onSubmit: (code: string) => void;
  onCancel: () => void;
}

/**
 * The pairing prompt. SRS §15, FR-PAIR-002/003.
 *
 * Design intent: the code shown here is compared *by the user* against the code
 * on the other computer's screen. The UI must present it large and unmissable,
 * and must make clear that a mismatch means something is wrong — that
 * comparison is the entire security value of the step.
 */
export function PairingDialog({
  challenge,
  secondsRemaining,
  busy,
  error,
  onSubmit,
  onCancel,
}: PairingDialogProps) {
  const [code, setCode] = useState("");
  const expired = secondsRemaining <= 0;

  // Clear the field when the challenge changes so a stale code from a previous
  // attempt is never submitted against a new one.
  useEffect(() => {
    setCode("");
  }, [challenge.deviceId]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="pairing-title"
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
    >
      <div className="w-full max-w-md rounded-lg border border-border bg-background p-5 shadow-lg">
        <h2 id="pairing-title" className="text-base font-semibold">
          Approve {challenge.displayName}
        </h2>

        <p className="mt-1 text-sm text-muted-foreground">
          {challenge.platform} · This device wants to connect.
        </p>

        <div className="mt-4 rounded-md bg-muted p-4 text-center">
          <p className="text-xs text-muted-foreground">Code on this computer</p>
          <p className="mt-1 font-mono text-3xl tracking-widest">{challenge.code}</p>
        </div>

        <p className="mt-3 text-sm text-muted-foreground">
          Check that {challenge.displayName} shows the same code. If it does not, press
          Cancel — something may be between the two computers.
        </p>

        {!expired && (
          <p className="mt-2 text-xs text-muted-foreground">
            Expires in {secondsRemaining}s
          </p>
        )}

        <form
          className="mt-4"
          onSubmit={(event) => {
            event.preventDefault();
            onSubmit(code);
          }}
        >
          <label htmlFor="pairing-code" className="block text-sm font-medium">
            Code shown on {challenge.displayName}
          </label>
          <input
            id="pairing-code"
            // `inputMode` gives a numeric keypad on touch devices without
            // preventing the user from typing the space.
            inputMode="numeric"
            autoComplete="off"
            autoFocus
            value={code}
            onChange={(event) => setCode(event.target.value)}
            placeholder="000 000"
            disabled={busy || expired}
            aria-invalid={error ? true : undefined}
            className={cx(
              "mt-1 w-full rounded-md border bg-background px-3 py-2 font-mono text-lg",
              "tracking-widest placeholder:text-muted-foreground",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary",
              error ? "border-destructive" : "border-border",
            )}
          />

          {error && (
            <p role="alert" className="mt-2 text-sm text-destructive">
              {error.what}
              {error.remedy ? ` ${error.remedy}` : ""}
            </p>
          )}

          <div className="mt-4 flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={onCancel} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" variant="primary" loading={busy} disabled={expired || !code}>
              Approve
            </Button>
          </div>
        </form>

        {error && error.kind !== "authenticationFailed" && (
          <div className="mt-3">
            <ErrorNotice error={error} />
          </div>
        )}
      </div>
    </div>
  );
}
