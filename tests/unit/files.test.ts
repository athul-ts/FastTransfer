import { describe, expect, it, vi } from "vitest";

// The dialog plugin needs a Tauri window; only the pure normaliser is tested.
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

import { toPathList } from "@/services/files";

describe("toPathList", () => {
  it("treats a cancelled picker as nothing chosen", () => {
    expect(toPathList(null)).toEqual([]);
    expect(toPathList(undefined)).toEqual([]);
  });

  it("wraps a single path", () => {
    expect(toPathList("C:\\Users\\a\\file.bin")).toEqual(["C:\\Users\\a\\file.bin"]);
  });

  it("keeps a list of paths and drops anything that is not a non-empty string", () => {
    expect(toPathList(["/a", "", 3, null, "/b"])).toEqual(["/a", "/b"]);
  });

  it("rejects other shapes", () => {
    expect(toPathList({ path: "/a" })).toEqual([]);
  });
});
