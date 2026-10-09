import type { AppError } from "@/types";
import { Button } from "@/components/shared/Button";

export interface ErrorNoticeProps {
  error: AppError;
  onRetry?: () => void;
  onResume?: () => void;
  onDismiss?: () => void;
}

/**
 * Renders an error the way SRS §95 requires: what happened, why, and what the
 * user can do about it.
 *
 * The `remedy` field is the contract, not decoration. An error reaching this
 * component without one is a bug in the Rust side's `error.rs` mapping, so the
 * component renders a fallback line rather than an empty space.
 */
export function ErrorNotice({ error, onRetry, onResume, onDismiss }: ErrorNoticeProps) {
  return (
    <div
      role="alert"
      className="rounded-md border border-destructive/40 bg-destructive/10 p-4 text-sm"
    >
      <p className="font-medium text-foreground">{error.what}</p>

      {error.why && <p className="mt-1 text-muted-foreground">{error.why}</p>}

      <p className="mt-1 text-muted-foreground">
        {error.remedy ?? "Try again, or check Settings > Advanced for details."}
      </p>

      <div className="mt-3 flex gap-2">
        {/* Resume only appears when the core says the data is intact — showing
            it speculatively would promise something we cannot deliver. */}
        {error.resumable && onResume && (
          <Button size="sm" variant="primary" onClick={onResume}>
            Resume
          </Button>
        )}
        {onRetry && (
          <Button size="sm" onClick={onRetry}>
            Retry
          </Button>
        )}
        {onDismiss && (
          <Button size="sm" variant="ghost" onClick={onDismiss}>
            Dismiss
          </Button>
        )}
      </div>
    </div>
  );
}
