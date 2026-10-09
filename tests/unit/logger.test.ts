import { describe, expect, it } from "vitest";

import { redact } from "@/utils/logger";

/** Walks into a redacted result along a key path, for readable assertions. */
function at(value: unknown, ...path: string[]): unknown {
  let cursor: unknown = value;
  for (const key of path) {
    if (typeof cursor !== "object" || cursor === null) return undefined;
    cursor = (cursor as Record<string, unknown>)[key];
  }
  return cursor;
}

/**
 * `redact` is the last line of defence behind CLAUDE.md security rule 5 —
 * logs never contain key material. These tests are deliberately adversarial:
 * they check the nested and aliased shapes that a careless `delete` on a
 * top-level field would miss.
 */
describe("redact", () => {
  it("passes primitives through untouched", () => {
    expect(redact("hello")).toBe("hello");
    expect(redact(42)).toBe(42);
    expect(redact(null)).toBeNull();
    expect(redact(true)).toBe(true);
  });

  it("strips pairing codes and secrets by key name", () => {
    const result = redact({ peer: "laptop", pairingCode: "123 456" });
    expect(at(result, "peer")).toBe("laptop");
    expect(at(result, "pairingCode")).toBe("[redacted]");
  });

  it("matches key names case-insensitively", () => {
    const result = redact({ PairingCode: "123 456", TOKEN: "abc" });
    expect(at(result, "PairingCode")).toBe("[redacted]");
    expect(at(result, "TOKEN")).toBe("[redacted]");
  });

  // A single-level redaction would leak the code from this shape.
  it("redacts at any depth", () => {
    const result = redact({
      session: { handshake: { secret: "s", proof: "p" }, peerId: "abc" },
    });
    expect(at(result, "session", "handshake", "secret")).toBe("[redacted]");
    expect(at(result, "session", "handshake", "proof")).toBe("[redacted]");
    expect(at(result, "session", "peerId")).toBe("abc");
  });

  it("redacts inside arrays", () => {
    const result = redact({ items: [{ token: "t" }, { name: "ok" }] });
    expect(at(result, "items", "0", "token")).toBe("[redacted]");
    expect(at(result, "items", "1", "name")).toBe("ok");
  });

  // File buffers must never reach a log line, whatever they are called.
  it("strips payload-bearing keys", () => {
    const result = redact({
      content: "file bytes",
      data: new Uint8Array([1, 2, 3]),
      bytes: 1024,
    });
    expect(at(result, "content")).toBe("[redacted]");
    expect(at(result, "data")).toBe("[redacted]");
    expect(at(result, "bytes")).toBe("[redacted]");
  });

  it("caps array length so one log line cannot carry a whole manifest", () => {
    const big = Array.from({ length: 500 }, (_, i) => i);
    expect(redact(big)).toHaveLength(50);
  });

  it("stops descending instead of recursing forever on a cycle", () => {
    const cyclic: Record<string, unknown> = { name: "root" };
    cyclic.self = cyclic;
    expect(() => redact(cyclic)).not.toThrow();
  });

  it("does not mutate its input", () => {
    const input = { pairingCode: "123 456" };
    redact(input);
    expect(input.pairingCode).toBe("123 456");
  });
});
