import "@testing-library/jest-dom/vitest";

import { vi } from "vitest";

/**
 * The Tauri IPC bridge does not exist in a test environment. Stub it so
 * modules under test can import `@tauri-apps/api` without exploding — the tests
 * that care about IPC behaviour mock these per-test.
 */
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

/**
 * `localStorage` is not guaranteed in every jsdom configuration and can throw
 * when storage is disabled. The app treats storage as best-effort, so tests
 * should exercise that path rather than assume it always works.
 */
if (!("localStorage" in globalThis)) {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
      removeItem: (key: string) => void store.delete(key),
      clear: () => store.clear(),
    },
    writable: true,
  });
}
