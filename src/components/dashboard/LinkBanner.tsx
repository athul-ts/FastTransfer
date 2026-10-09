import type { LinkInfo } from "@/types";
import { cx, formatSpeed, linkLabel } from "@/utils/helpers";

export interface LinkBannerProps {
  link: LinkInfo | null;
  /** Link rating in bits/second, when the OS reports one. */
  linkSpeedBps?: number | null;
}

/**
 * The strip at the top of the main screen saying what the cable can do.
 *
 * This is where the product's honesty principle is most visible (SRS §101).
 * It shows the **link rating** and the **measured throughput** as two separate
 * figures, labelled, so "40 Gbps" can never be read as a transfer speed.
 *
 * When the link is unusable it shows the explanation from the Rust side rather
 * than a generic failure — a USB data-only cable is the case users find most
 * confusing, and it has a specific, useful answer (SRS FR-LINK-007).
 */
export function LinkBanner({ link, linkSpeedBps }: LinkBannerProps) {
  if (!link) {
    return (
      <div className="border-b border-border px-4 py-2 text-sm text-muted-foreground">
        Checking connection…
      </div>
    );
  }

  const { kind, explanation, measuredBps } = link;
  const usable = explanation.usable;

  return (
    <div
      className={cx(
        "border-b px-4 py-2 text-sm",
        usable ? "border-border" : "border-destructive/40 bg-destructive/10",
      )}
    >
      <div className="flex flex-wrap items-baseline gap-x-3 gap-y-1">
        <span className="font-medium">{linkLabel(kind)}</span>

        {usable && linkSpeedBps ? (
          <>
            <span className="text-muted-foreground">
              Link: <span className="text-foreground">{formatSpeed(measuredBps)}</span>
            </span>
            <span className="text-muted-foreground text-xs">
              (rated {(linkSpeedBps / 1_000_000_000).toFixed(0)} Gbps)
            </span>
          </>
        ) : (
          <span className="text-muted-foreground">{explanation.detail}</span>
        )}
      </div>

      {!usable && explanation.remedy && (
        <p className="mt-1 text-muted-foreground">{explanation.remedy}</p>
      )}

      {/* When nothing has been measured yet, say so rather than showing a zero
          that reads as "stalled". */}
      {usable && measuredBps === null && (
        <p className="mt-1 text-xs text-muted-foreground">
          Transfer speed has not been measured yet.
        </p>
      )}
    </div>
  );
}
