// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vite-plus/test";
import { cleanupLegacyWorkers } from "./legacyWorkers";

describe("legacy service worker cleanup", () => {
  it("keeps only the dedicated push worker while removing old UI workers", () => {
    const registration = (path: string, script: string) => ({
      scope: new URL(path, window.location.origin).href,
      active: { scriptURL: new URL(script, window.location.origin).href },
      waiting: null,
      installing: null,
      unregister: vi.fn().mockResolvedValue(true),
      update: vi.fn().mockResolvedValue(undefined),
    });
    const push = registration("/ui/push/", "/ui/push/sw.js");
    const old = registration("/ui/", "/ui/sw.js");
    const impersonator = registration("/ui/push/", "/ui/other.js");
    const unrelated = registration("/other/", "/other/sw.js");
    cleanupLegacyWorkers([
      push,
      old,
      impersonator,
      unrelated,
    ] as unknown as ServiceWorkerRegistration[]);
    expect(push.unregister).not.toHaveBeenCalled();
    expect(push.update).toHaveBeenCalledOnce();
    expect(old.unregister).toHaveBeenCalledOnce();
    expect(impersonator.unregister).toHaveBeenCalledOnce();
    expect(unrelated.unregister).not.toHaveBeenCalled();
  });
});
