/**
 * The OS file and folder pickers (FR-TRANSFER-001, FR-TRANSFER-002).
 *
 * Paths returned here were chosen by the local user and are trusted. They go
 * to `start_transfer` as they are; the core builds the manifest. Paths from a
 * peer never come through this module (SRS §91).
 */

import type { UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

/** Normalises whatever the picker returned into a list of paths. */
export function toPathList(selection: unknown): string[] {
  if (typeof selection === "string") return selection ? [selection] : [];
  if (Array.isArray(selection)) {
    return selection.filter((p): p is string => typeof p === "string" && p.length > 0);
  }
  return [];
}

/** Lets the user pick one or more files. Empty when they cancel. */
export async function pickFiles(): Promise<string[]> {
  return toPathList(await open({ multiple: true, directory: false, title: "Choose files to send" }));
}

/** Lets the user pick one folder. Empty when they cancel. */
export async function pickFolder(): Promise<string[]> {
  return toPathList(await open({ multiple: false, directory: true, title: "Choose a folder to send" }));
}

/**
 * Calls `handler` with the paths of files or folders dropped on the window
 * (FR-TRANSFER-003). Returns the unlisten function for effect cleanup.
 */
export function onFilesDropped(handler: (paths: string[]) => void): Promise<UnlistenFn> {
  return getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop") {
      const paths = toPathList(event.payload.paths);
      if (paths.length > 0) handler(paths);
    }
  });
}
