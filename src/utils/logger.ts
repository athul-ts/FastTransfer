/**
 * Frontend logger.
 *
 * Structured, levelled, and — critically — it never receives file contents or
 * key material. See CLAUDE.md § Security rules and SRS NFR-SEC-007.
 *
 * Errors from the Rust core are already structured; this logs them by tag so a
 * support log is readable without the internals leaking into the UI.
 */

export type LogLevel = "error" | "warn" | "info" | "debug" | "trace";

const LEVELS: Record<LogLevel, number> = {
  error: 0,
  warn: 1,
  info: 2,
  debug: 3,
  trace: 4,
};

function configuredLevel(): LogLevel {
  const raw = import.meta.env.VITE_LOG_LEVEL as string | undefined;
  if (raw && raw in LEVELS) return raw as LogLevel;
  return import.meta.env.DEV ? "debug" : "info";
}

/**
 * Keys whose values must never be logged. Matching is by key name anywhere in
 * a logged object, so a nested `{ peer: { pairingCode } }` is caught too.
 */
const REDACTED_KEYS = new Set([
  "code",
  "pairingcode",
  "secret",
  "key",
  "privatekey",
  "publickey",
  "token",
  "password",
  "proof",
  "content",
  "contents",
  "data",
  "bytes",
]);

/** Recursively strips values under any redacted key. */
export function redact(value: unknown, depth = 0): unknown {
  if (depth > 6) return "[deep]";
  if (value === null || typeof value !== "object") return value;

  if (Array.isArray(value)) {
    return value.slice(0, 50).map((v) => redact(v, depth + 1));
  }

  const out: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(value as Record<string, unknown>)) {
    out[key] = REDACTED_KEYS.has(key.toLowerCase()) ? "[redacted]" : redact(val, depth + 1);
  }
  return out;
}

function emit(level: LogLevel, scope: string, message: string, context?: unknown): void {
  if (LEVELS[level] > LEVELS[configuredLevel()]) return;

  const line = `[fasttransfer:${scope}] ${message}`;
  const payload = context === undefined ? undefined : redact(context);

  switch (level) {
    case "error":
      console.error(line, payload);
      break;
    case "warn":
      console.warn(line, payload);
      break;
    case "debug":
      console.debug(line, payload);
      break;
    default:
      console.log(line, payload);
  }
}

export interface Logger {
  error(message: string, context?: unknown): void;
  warn(message: string, context?: unknown): void;
  info(message: string, context?: unknown): void;
  debug(message: string, context?: unknown): void;
  trace(message: string, context?: unknown): void;
  /** A logger scoped to a module, e.g. `logger.withScope("transfer")`. */
  withScope(scope: string): Logger;
}

function createLogger(scope: string): Logger {
  return {
    error: (m, c) => emit("error", scope, m, c),
    warn: (m, c) => emit("warn", scope, m, c),
    info: (m, c) => emit("info", scope, m, c),
    debug: (m, c) => emit("debug", scope, m, c),
    trace: (m, c) => emit("trace", scope, m, c),
    withScope: (child) => createLogger(`${scope}:${child}`),
  };
}

export const logger = createLogger("app");
