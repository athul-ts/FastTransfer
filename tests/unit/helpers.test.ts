import { describe, expect, it } from "vitest";

import {
  clamp,
  cx,
  formatBytes,
  formatDuration,
  formatLinkSpeed,
  formatSpeed,
  fraction,
  linkLabel,
} from "@/utils/helpers";

describe("formatBytes", () => {
  it("formats binary units", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1024)).toBe("1.0 KB");
    expect(formatBytes(1024 ** 2)).toBe("1.0 MB");
    expect(formatBytes(1024 ** 3)).toBe("1.0 GB");
  });

  it("returns a dash for invalid input rather than NaN", () => {
    expect(formatBytes(Number.NaN)).toBe("—");
    expect(formatBytes(-1)).toBe("—");
    expect(formatBytes(Number.POSITIVE_INFINITY)).toBe("—");
  });
});

describe("formatSpeed", () => {
  // SRS §101: an unmeasured speed must read as "not measured", never as zero,
  // which a user would read as "stalled".
  it("returns a dash when there is no measurement", () => {
    expect(formatSpeed(null)).toBe("—");
    expect(formatSpeed(undefined)).toBe("—");
    expect(formatSpeed(0)).toBe("—");
  });

  it("formats a measured rate", () => {
    expect(formatSpeed(1024 ** 3)).toBe("1.00 GB/s");
  });
});

describe("formatLinkSpeed", () => {
  // Link ratings are decimal bits per second — a different unit from the
  // binary throughput figure, and kept visibly distinct.
  it("uses decimal bit units", () => {
    expect(formatLinkSpeed(1_000_000_000)).toBe("1 Gbps");
    expect(formatLinkSpeed(40_000_000_000)).toBe("40 Gbps");
    expect(formatLinkSpeed(2_500_000_000)).toBe("2.5 Gbps");
  });

  it("handles an unknown link rating", () => {
    expect(formatLinkSpeed(null)).toBe("—");
    expect(formatLinkSpeed(0)).toBe("—");
  });
});

describe("formatDuration", () => {
  it("omits the hour field under an hour", () => {
    expect(formatDuration(0)).toBe("00:00");
    expect(formatDuration(59)).toBe("00:59");
    expect(formatDuration(61)).toBe("01:01");
  });

  it("includes hours when needed", () => {
    expect(formatDuration(3661)).toBe("1:01:01");
  });

  it("returns a dash for an unknown estimate", () => {
    expect(formatDuration(null)).toBe("—");
    expect(formatDuration(Number.NaN)).toBe("—");
  });
});

describe("fraction", () => {
  // A zero-byte transfer (an empty folder) must never render as NaN%.
  it("guards the zero-total case", () => {
    expect(fraction(0, 0)).toBe(0);
    expect(Number.isNaN(fraction(0, 0))).toBe(false);
  });

  it("clamps to 0..1", () => {
    expect(fraction(150, 100)).toBe(1);
    expect(fraction(-10, 100)).toBe(0);
    expect(fraction(50, 100)).toBe(0.5);
  });
});

describe("clamp", () => {
  it("clamps into range", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-5, 0, 10)).toBe(0);
    expect(clamp(50, 0, 10)).toBe(10);
  });
});

describe("cx", () => {
  it("joins truthy class names only", () => {
    expect(cx("a", false, null, undefined, "b")).toBe("a b");
    expect(cx()).toBe("");
  });
});

describe("linkLabel", () => {
  it("maps every kind to a user-facing label", () => {
    expect(linkLabel("THUNDERBOLT")).toBe("Thunderbolt");
    expect(linkLabel("USB4_NETWORK")).toBe("USB4");
    expect(linkLabel("USB_DATA_ONLY")).toBe("USB-C (data only)");
    expect(linkLabel("CHARGE_ONLY")).toBe("USB-C (charging only)");
    expect(linkLabel("NONE")).toBe("No cable");
  });

  it("passes through an unknown kind rather than showing undefined", () => {
    expect(linkLabel("SOMETHING_NEW")).toBe("SOMETHING_NEW");
  });
});
