// @vitest-environment happy-dom

import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import {
  claimVoiceCaptureLock,
  createVoiceRecordingSessionId,
  releaseVoiceCaptureLock,
} from "./voiceCaptureLock";

const LOCK_KEY = "cccc.voiceSecretary.activeCapture";

afterEach(() => {
  window.localStorage.clear();
  vi.unstubAllGlobals();
});

describe("voice recording session identity", () => {
  it("creates a fresh identity for every recording", () => {
    const first = createVoiceRecordingSessionId();
    const second = createVoiceRecordingSessionId();
    expect(first).toMatch(/^voice-session-/);
    expect(second).toMatch(/^voice-session-/);
    expect(second).not.toBe(first);
  });
});

describe("voice capture lock", () => {
  it("reclaims an unprobeable advisory lock instead of false-blocking recording", async () => {
    window.localStorage.setItem(
      LOCK_KEY,
      JSON.stringify({ ownerId: "old-tab", groupId: "g-old", updatedAt: Date.now() }),
    );
    vi.stubGlobal("BroadcastChannel", undefined);

    await expect(claimVoiceCaptureLock("new-tab", "g-new")).resolves.toBeNull();
    expect(JSON.parse(window.localStorage.getItem(LOCK_KEY) || "{}")).toMatchObject({
      ownerId: "new-tab",
      groupId: "g-new",
    });
  });

  it("reclaims an expired lock", async () => {
    window.localStorage.setItem(
      LOCK_KEY,
      JSON.stringify({ ownerId: "old-tab", groupId: "g-old", updatedAt: Date.now() - 31_000 }),
    );

    await expect(claimVoiceCaptureLock("new-tab", "g-new")).resolves.toBeNull();
    expect(JSON.parse(window.localStorage.getItem(LOCK_KEY) || "{}").ownerId).toBe("new-tab");
  });

  it("keeps a lock when its owning tab answers the probe", async () => {
    class AliveChannel extends EventTarget {
      postMessage(message: { type?: string; ownerId?: string }) {
        if (message.type !== "probe") return;
        queueMicrotask(() =>
          this.dispatchEvent(
            new MessageEvent("message", {
              data: { type: "alive", ownerId: message.ownerId, groupId: "g-old" },
            }),
          ),
        );
      }
      close() {}
    }
    vi.stubGlobal("BroadcastChannel", AliveChannel);
    window.localStorage.setItem(
      LOCK_KEY,
      JSON.stringify({ ownerId: "old-tab", groupId: "g-old", updatedAt: Date.now() }),
    );

    await expect(claimVoiceCaptureLock("new-tab", "g-new")).resolves.toMatchObject({
      ownerId: "old-tab",
      groupId: "g-old",
    });
    releaseVoiceCaptureLock("old-tab");
  });
});
