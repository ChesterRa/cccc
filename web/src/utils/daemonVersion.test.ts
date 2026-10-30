import { describe, expect, it } from "vite-plus/test";
import { daemonVersionDifference } from "./daemonVersion";

describe("daemonVersionDifference", () => {
  it("reports a known version difference without deciding which is compatible", () => {
    expect(daemonVersionDifference({ version: "0.4.45", daemon: { version: "0.4.44" } })).toEqual({
      webVersion: "0.4.45",
      daemonVersion: "0.4.44",
    });
    expect(daemonVersionDifference({ version: "0.4.44", daemon: { version: "0.4.45" } })).toEqual({
      webVersion: "0.4.44",
      daemonVersion: "0.4.45",
    });
    expect(
      daemonVersionDifference({ version: "1.0.0-rc2", daemon: { version: "1.0.0+build.4" } }),
    ).toEqual({ webVersion: "1.0.0-rc2", daemonVersion: "1.0.0+build.4" });
  });

  it("treats absent or malformed versions as unknown, and matching versions as equal", () => {
    for (const value of [
      null,
      {},
      { version: "0.4.45" },
      { version: "0.4.45", daemon: null },
      { version: "0.4.45", daemon: [] },
      { version: "0.4.45", daemon: { version: "" } },
      { version: "0.4.45", daemon: { version: 444 } },
      { version: "0.4.45", daemon: { version: "unknown" } },
      { version: "0.4.45", daemon: { version: "0.4.45\nnotification" } },
      { version: "0.4.45", daemon: { version: "0.4.45" } },
    ]) {
      expect(daemonVersionDifference(value)).toBeNull();
    }
  });
});
