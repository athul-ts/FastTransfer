/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_LOG_LEVEL?: "error" | "warn" | "info" | "debug" | "trace";
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
