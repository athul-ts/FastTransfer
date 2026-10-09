/**
 * Formatting and small pure helpers.
 *
 * The byte and duration formatters are load-bearing for the product's honesty
 * principle (SRS §101): the UI shows measured throughput, and it must never
 * round a measurement up into a claim the link cannot support.
 */

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"] as const;

/**
 * Formats a byte count with binary units.
 *
 * Uses 1024-based units, which is what the transfer engine actually moves.
 * `1.00 GB` here means 1073741824 bytes, matching what the user sees in their
 * file manager on Linux and Windows.
 */
export function formatBytes(bytes: number, decimals = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes === 0) return "0 B";

  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }

  return unit === 0
    ? `${bytes} B`
    : `${value.toFixed(decimals)} ${BYTE_UNITS[unit]}`;
}

/**
 * Formats a throughput figure.
 *
 * Returns "—" rather than "0 B/s" when there is no measurement yet, so the UI
 * shows "not measured" instead of implying the transfer is stalled.
 */
export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  if (bytesPerSecond === null || bytesPerSecond === undefined) return "—";
  if (!Number.isFinite(bytesPerSecond) || bytesPerSecond < 0) return "—";
  if (bytesPerSecond === 0) return "—";
  return `${formatBytes(bytesPerSecond, 2)}/s`;
}

/**
 * Formats a link rating in bits per second — a different unit from throughput,
 * and kept visibly separate so the two are never confused.
 */
export function formatLinkSpeed(bitsPerSecond: number | null | undefined): string {
  if (!bitsPerSecond) return "—";

  const units = ["bps", "Kbps", "Mbps", "Gbps", "Tbps"];
  let value = bitsPerSecond;
  let unit = 0;
  while (value >= 1000 && unit < units.length - 1) {
    value /= 1000;
    unit += 1;
  }
  return `${Number.isInteger(value) ? value : value.toFixed(1)} ${units[unit]}`;
}

/** Formats a duration as `HH:MM:SS`, or `MM:SS` under an hour. */
export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "—";
  if (!Number.isFinite(seconds) || seconds < 0) return "—";

  const total = Math.floor(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;

  const pad = (n: number) => n.toString().padStart(2, "0");
  return hours > 0
    ? `${hours}:${pad(minutes)}:${pad(secs)}`
    : `${pad(minutes)}:${pad(secs)}`;
}

/** Clamps a number into a range. */
export function clamp(value: number, min: number, max: number): number {
  return Math.min(Math.max(value, min), max);
}

/**
 * Completion fraction in `0..=1`, guarding the zero-byte case.
 *
 * A zero-byte transfer (an empty folder) must render as 0% or 100%, never NaN.
 */
export function fraction(transferred: number, total: number): number {
  if (!Number.isFinite(total) || total <= 0) return 0;
  return clamp(transferred / total, 0, 1);
}

/** Joins class names, dropping falsy entries. */
export function cx(...classes: Array<string | false | null | undefined>): string {
  return classes.filter(Boolean).join(" ");
}

/** Formats a Unix timestamp (seconds) as a local time. */
export function formatTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
}

/**
 * Human label for a link kind. Kept here rather than in the component so the
 * diagnostics screen and the device card cannot drift apart.
 */
export function linkLabel(kind: string): string {
  switch (kind) {
    case "THUNDERBOLT":
      return "Thunderbolt";
    case "USB4_NETWORK":
      return "USB4";
    case "USB_DATA_ONLY":
      return "USB-C (data only)";
    case "CHARGE_ONLY":
      return "USB-C (charging only)";
    case "NONE":
      return "No cable";
    default:
      return kind;
  }
}
