/**
 * Shared types.
 *
 * These mirror the Rust types that cross the Tauri IPC boundary. When a Rust
 * struct in `src-tauri/src/` changes, the matching type here must change in the
 * same commit — a mismatch compiles and then fails at runtime.
 *
 * | Type | Rust source |
 * |---|---|
 * | `LinkKind`, `InterfaceKind`, `LinkExplanation`, `LinkInfo` | `link.rs`, `commands.rs` |
 * | `Peer`, `DiscoveryMethod` | `discovery.rs` |
 * | `TransportCandidate`, `TransportKind`, `Interface` | `transport.rs` |
 * | `Capabilities`, `ProtocolVersion` | `protocol.rs` |
 * | `TransferStatus`, `TransferProgress` | `transfer.rs` |
 * | `TrustedDevice`, `PairingChallenge` | `security.rs`, `commands.rs` |
 * | `AppError` | `error.rs::ErrorPayload` |
 */

// ---------------------------------------------------------------------------
// Link
// ---------------------------------------------------------------------------

/** What a plugged cable can actually do. Mirrors `link::LinkKind`. */
export type LinkKind =
  | "THUNDERBOLT"
  | "USB4_NETWORK"
  | "USB_DATA_ONLY"
  | "CHARGE_ONLY"
  | "NONE";

export interface LinkExplanation {
  headline: string;
  detail: string;
  remedy: string | null;
  usable: boolean;
}

export interface LinkInfo {
  kind: LinkKind;
  explanation: LinkExplanation;
  /** Measured throughput. Never the link rating — see SRS §101. */
  measuredBps: number | null;
}

/**
 * What a network interface physically is. Mirrors `link::InterfaceKind`.
 *
 * Broader than `LinkKind`: Ethernet and Wi-Fi are not cables, so they have no
 * `LinkKind`. Ranking an interface by its `LinkKind` is what made both fallback
 * paths unreachable, so the two types are kept deliberately separate.
 */
export type InterfaceKind =
  | "THUNDERBOLT"
  | "USB4_NETWORK"
  | "ETHERNET"
  | "WIFI"
  | "USB_DATA_ONLY"
  | "OTHER";

export interface NetworkInterface {
  name: string;
  kind: InterfaceKind;
  linkSpeedBps: number | null;
  up: boolean;
  isVirtual: boolean;
  addresses: string[];
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

export type DiscoveryMethod = "mdns" | "linkLocalProbe";

export interface ProtocolVersion {
  major: number;
  minor: number;
}

export interface Capabilities {
  protocol: ProtocolVersion;
  maxChunkSize: number;
  resume: boolean;
  sha256: boolean;
  compression: boolean;
  encryption: boolean;
  transports: string[];
}

export interface Peer {
  deviceId: string;
  displayName: string;
  platform: string;
  platformVersion: string;
  appVersion: string;
  protocolVersion: string;
  capabilities: Capabilities;
  addresses: string[];
  port: number;
  /** How we found it. Shown in diagnostics so a failure is explainable. */
  via: DiscoveryMethod;
  link: LinkKind;
  trusted: boolean;
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

export type TransportKind = "thunderbolt" | "ethernet" | "wifi";

export interface TransportCandidate {
  kind: TransportKind;
  interface: NetworkInterface;
  score: number;
  measuredBps: number | null;
}

// ---------------------------------------------------------------------------
// Transfer
// ---------------------------------------------------------------------------

export type TransferStatus =
  | "QUEUED"
  | "PREPARING"
  | "CONNECTING"
  | "AUTHENTICATING"
  | "TRANSFERRING"
  | "VERIFYING"
  | "COMPLETED"
  | "PAUSED"
  | "INTERRUPTED"
  | "FAILED"
  | "CANCELLED";

export interface TransferProgress {
  transferId: string;
  status: TransferStatus;
  totalBytes: number;
  transferredBytes: number;
  /** Measured, not derived from the link rating. */
  currentBps: number;
  averageBps: number;
  etaSeconds: number | null;
  currentFile: string | null;
}

export type ConflictResolution = "replace" | "skip" | "keepBoth" | "cancel";

// ---------------------------------------------------------------------------
// Pairing and trust
// ---------------------------------------------------------------------------

export interface TrustedDevice {
  deviceId: string;
  displayName: string;
  platform: string;
  publicKey: string;
  trustedSince: number;
  lastSeen: number;
}

export interface PairingChallenge {
  deviceId: string;
  displayName: string;
  platform: string;
  /** Show this. The user compares it with the other screen. */
  code: string;
  expiresInSeconds: number;
}

// ---------------------------------------------------------------------------
// Diagnostics and settings
// ---------------------------------------------------------------------------

export interface Diagnostics {
  deviceId: string;
  displayName: string;
  appVersion: string;
  platform: string;
  link: LinkInfo;
  interfaces: NetworkInterface[];
  transports: TransportCandidate[];
  peerCount: number;
  discoveryWorking: boolean;
  trustedCount: number;
  pendingPairings: number;
  /** Plain-language notes about likely problems. */
  notes: string[];
}

export interface ApplicationSettings {
  deviceName: string;
  downloadDirectory: string;
  conflictResolution: ConflictResolution;
  verifyAfterTransfer: boolean;
  resumeAutomatically: boolean;
  preferredTransport: string | null;
  notificationsEnabled: boolean;
  theme: "light" | "dark" | "system";
  logLevel: "error" | "warn" | "info" | "debug" | "trace";
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/** Machine-readable error tags. Mirrors `error.rs::ErrorPayload::kind`. */
export type AppErrorKind =
  | "linkUnusable"
  | "noTransport"
  | "peerNotFound"
  | "connectionLost"
  | "peerRejected"
  | "authenticationFailed"
  | "versionMismatch"
  | "protocolError"
  | "unsafePath"
  | "destinationNotWritable"
  | "diskFull"
  | "verificationFailed"
  | "transferNotFound"
  | "cancelled"
  | "io"
  | "internal";

/**
 * The structured error the Rust core returns.
 *
 * Every error answers what happened and, where known, why and what to do next
 * (SRS §95). The UI must never render a bare `ECONNRESET`.
 */
export interface AppError {
  kind: AppErrorKind;
  what: string;
  why: string | null;
  remedy: string | null;
  /** Whether a Resume affordance is valid for this failure. */
  resumable: boolean;
}

export function isAppError(value: unknown): value is AppError {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Record<string, unknown>;
  return (
    typeof candidate.kind === "string" &&
    typeof candidate.what === "string" &&
    typeof candidate.resumable === "boolean"
  );
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/** Event names emitted by the Rust core. Mirrors SRS §79. */
export const Events = {
  deviceDiscovered: "device_discovered",
  deviceUpdated: "device_updated",
  deviceRemoved: "device_removed",
  linkChanged: "link_changed",
  transferStarted: "transfer_started",
  transferProgress: "transfer_progress",
  transferPaused: "transfer_paused",
  transferResumed: "transfer_resumed",
  transferCompleted: "transfer_completed",
  transferFailed: "transfer_failed",
  transferCancelled: "transfer_cancelled",
  connectionChanged: "connection_changed",
  diagnosticUpdate: "diagnostic_update",
} as const;

export type EventName = (typeof Events)[keyof typeof Events];
