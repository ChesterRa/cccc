// @vitest-environment happy-dom
import { describe, it, expect, vi } from "vite-plus/test";
import { buildMismatch, loadedWebEntry } from "./runtimeBuildInfo";

describe("effective build diagnostics", () => {
  const same = {
    webSource: "a",
    daemonSource: "a",
    webAssets: "bundle",
    servedEntry: "/ui/assets/one.js",
    loadedEntry: "/ui/assets/one.js",
  };
  it("detects same-version source and loaded asset differences independently", () => {
    expect(buildMismatch(same)).toBe(false);
    expect(buildMismatch({ ...same, daemonSource: "b" })).toBe(true);
    expect(buildMismatch({ ...same, loadedEntry: "/ui/assets/old.js" })).toBe(true);
    expect(buildMismatch({ ...same, loadedEntry: "/ui/src/main.tsx" })).toBe(false);
    expect(buildMismatch({ ...same, daemonSource: "", servedEntry: "" })).toBe(false);
    expect(buildMismatch()).toBe(false);
  });
  it("copies only the actual entry path, without origin, query or fragment", () => {
    const script = document.createElement("script");
    script.type = "module";
    script.src = "https://private.example/ui/assets/entry.js?access_token=secret#private";
    const query = vi.spyOn(document, "querySelector").mockReturnValue(script);
    try {
      expect(loadedWebEntry()).toBe("/ui/assets/entry.js");
    } finally {
      query.mockRestore();
    }
  });
});
