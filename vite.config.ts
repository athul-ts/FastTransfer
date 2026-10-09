import { fileURLToPath } from "node:url";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

// Tauri sets TAURI_DEV_HOST when developing against a physical device.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],

  resolve: {
    // `__dirname` does not exist here — package.json sets `"type": "module"`,
    // so this config is ESM.
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  // Tauri prints its own output; don't let Vite wipe it.
  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    // The Rust side has its own watcher.
    watch: { ignored: ["**/src-tauri/**"] },
  },

  envPrefix: ["VITE_", "TAURI_ENV_"],

  build: {
    // Tauri uses WebView2 on Windows and WKWebView elsewhere.
    target: process.env.TAURI_ENV_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },

  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
    include: ["tests/**/*.test.{ts,tsx}"],
  },
});
