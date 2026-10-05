import { describe, expect, it } from "vite-plus/test";
import { runtimeDetectionKey } from "./runtimeDetection";
import type { RuntimeInfo } from "../types";

describe("host Runtime detection", () => {
  const info: RuntimeInfo = {
    name: "antigravity",
    display_name: "Antigravity",
    available: false,
    mode_availability: { default: false, acp: true },
  };
  it("distinguishes ACP installation from native CLI detection", () => {
    expect(runtimeDetectionKey("antigravity", info, "ready", "acp")).toBe("detected");
    expect(runtimeDetectionKey("antigravity", info, "ready", "default")).toBe("missing");
    const nativeOnly = {
      ...info,
      available: true,
      mode_availability: { default: true, acp: false },
    };
    expect(runtimeDetectionKey("antigravity", nativeOnly, "ready", "acp")).toBe("missing");
    expect(runtimeDetectionKey("antigravity", nativeOnly, "ready")).toBe("detected");
  });
  it("does not call incomplete, loading or failed detection a missing installation", () => {
    for (const status of ["idle", "loading"] as const)
      expect(runtimeDetectionKey("codex", undefined, status)).toBe("checking");
    expect(runtimeDetectionKey("codex", undefined, "error")).toBe("failed");
    expect(runtimeDetectionKey("codex", undefined, "ready")).toBe("unknown");
    expect(
      runtimeDetectionKey("antigravity", { ...info, mode_availability: undefined }, "ready", "acp"),
    ).toBe("unknown");
    expect(runtimeDetectionKey("antigravity", info, "error", "acp")).toBe("failed");
  });
  it("separates Web and custom configuration from local installation", () => {
    for (const runtime of ["web_model", "grok_web_model"])
      expect(runtimeDetectionKey(runtime, undefined, "ready")).toBe("web");
    expect(runtimeDetectionKey("custom", undefined, "ready")).toBe("custom");
  });
});
