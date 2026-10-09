import { describe, expect, it } from "vitest";

import {
  asAppError,
  asNumber,
  asString,
  fileNameIssue,
  isPeer,
  isTransferProgress,
  normalizePairingCode,
} from "@/utils/validators";

describe("normalizePairingCode", () => {
  it("accepts the shapes a user actually types", () => {
    expect(normalizePairingCode("123456")).toBe("123 456");
    expect(normalizePairingCode("123 456")).toBe("123 456");
    expect(normalizePairingCode("123-456")).toBe("123 456");
    expect(normalizePairingCode("  123 456  ")).toBe("123 456");
  });

  it("rejects anything that is not six digits", () => {
    expect(normalizePairingCode("12345")).toBeNull();
    expect(normalizePairingCode("1234567")).toBeNull();
    expect(normalizePairingCode("12345a")).toBeNull();
    expect(normalizePairingCode("")).toBeNull();
  });

  it("does not accept unicode digits that a regex shortcut might allow", () => {
    // Full-width digits are visually identical but must not pass as a code.
    expect(normalizePairingCode("１２３４５６")).toBeNull();
  });
});

describe("asString / asNumber", () => {
  it("narrows only the matching primitive", () => {
    expect(asString("x")).toBe("x");
    expect(asString(1)).toBeNull();
    expect(asNumber(1)).toBe(1);
    expect(asNumber("1")).toBeNull();
  });

  it("rejects non-finite numbers", () => {
    expect(asNumber(Number.NaN)).toBeNull();
    expect(asNumber(Number.POSITIVE_INFINITY)).toBeNull();
  });
});

describe("isPeer", () => {
  const valid = {
    deviceId: "abc",
    displayName: "Laptop",
    link: "THUNDERBOLT",
    addresses: ["169.254.1.2"],
  };

  it("accepts a complete peer", () => {
    expect(isPeer(valid)).toBe(true);
  });

  it("rejects a peer missing any field the card renders", () => {
    for (const key of ["deviceId", "displayName", "link", "addresses"]) {
      const partial: Record<string, unknown> = { ...valid };
      delete partial[key];
      expect(isPeer(partial)).toBe(false);
    }
  });

  it("rejects non-objects and null", () => {
    expect(isPeer(null)).toBe(false);
    expect(isPeer("peer")).toBe(false);
    expect(isPeer([])).toBe(false);
  });
});

describe("isTransferProgress", () => {
  it("accepts a progress payload with the fields the bar needs", () => {
    expect(
      isTransferProgress({
        transferId: "t1",
        status: "TRANSFERRING",
        totalBytes: 100,
        transferredBytes: 40,
      }),
    ).toBe(true);
  });

  it("rejects a payload with the byte counts as strings", () => {
    expect(
      isTransferProgress({
        transferId: "t1",
        status: "TRANSFERRING",
        totalBytes: "100",
        transferredBytes: 40,
      }),
    ).toBe(false);
  });
});

describe("fileNameIssue", () => {
  it("rejects a name that is empty once trimmed", () => {
    expect(fileNameIssue("   ")).not.toBeNull();
  });

  it("catches Windows-reserved characters", () => {
    expect(fileNameIssue('a<b.txt')).not.toBeNull();
    expect(fileNameIssue("a|b.txt")).not.toBeNull();
  });

  it("catches a trailing dot or space, which Windows silently strips", () => {
    expect(fileNameIssue("report.")).not.toBeNull();
    expect(fileNameIssue("report ")).not.toBeNull();
  });

  it("catches reserved device names, with and without an extension", () => {
    expect(fileNameIssue("CON")).not.toBeNull();
    expect(fileNameIssue("con.txt")).not.toBeNull();
    expect(fileNameIssue("LPT1.log")).not.toBeNull();
  });

  it("permits ordinary names, including ones that merely start like a device", () => {
    expect(fileNameIssue("report.txt")).toBeNull();
    expect(fileNameIssue("console.log")).toBeNull();
    expect(fileNameIssue("CONcert.mp4")).toBeNull();
  });
});

describe("asAppError", () => {
  it("passes through an already-structured error", () => {
    const error = {
      kind: "permissionDenied" as const,
      what: "Cannot write there.",
      why: null,
      remedy: "Choose another folder.",
      resumable: false,
    };
    expect(asAppError(error)).toBe(error);
  });

  // A panic or a rejected promise from the runtime arrives as a bare string.
  // It must still render as a complete three-part message.
  it("wraps an unstructured throw into something renderable", () => {
    const error = asAppError(new Error("thread panicked"));
    expect(error.kind).toBe("internal");
    expect(error.what).toBeTruthy();
    expect(error.remedy).toBeTruthy();
    expect(error.resumable).toBe(false);
  });

  it("wraps a bare string throw", () => {
    expect(asAppError("boom").why).toBe("boom");
    expect(asAppError(undefined).what).toBeTruthy();
  });
});
