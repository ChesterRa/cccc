// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
const load = () => import("./useVoiceAudioStore");
beforeEach(() => {
  localStorage.clear();
  vi.resetModules();
});
afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
});

it("preserves the existing Codex device choice once and keeps it independent of speaking voice", async () => {
  localStorage.setItem(
    "cccc.codexVoice.preferences.v1",
    JSON.stringify({
      voice: "sol",
      inputDeviceId: "original-mic",
      outputDeviceId: "original-speaker",
    }),
  );
  const audio = await load();
  expect(audio.voiceAudioSnapshot()).toEqual({
    inputDeviceId: "original-mic",
    outputDeviceId: "original-speaker",
  });
  audio.useVoiceAudioStore.getState().update({ inputDeviceId: "shared-mic" });
  localStorage.setItem(
    "cccc.codexVoice.preferences.v1",
    JSON.stringify({ voice: "cove", inputDeviceId: "stale-device" }),
  );
  vi.resetModules();
  expect((await load()).voiceAudioSnapshot()).toEqual({
    inputDeviceId: "shared-mic",
    outputDeviceId: "original-speaker",
  });
});

it("reads the latest saved devices for new starts while an active snapshot remains unchanged", async () => {
  const audio = await load();
  audio.useVoiceAudioStore
    .getState()
    .update({ inputDeviceId: "mic-a", outputDeviceId: "speaker-a" });
  const active = audio.voiceAudioSnapshot();
  localStorage.setItem(
    audio.VOICE_AUDIO_STORAGE_KEY,
    JSON.stringify({ inputDeviceId: "mic-b", outputDeviceId: "speaker-b" }),
  );
  expect(audio.voiceAudioSnapshot()).toEqual({
    inputDeviceId: "mic-b",
    outputDeviceId: "speaker-b",
  });
  expect(active).toEqual({ inputDeviceId: "mic-a", outputDeviceId: "speaker-a" });
});

it("shares cross-tab updates and removes listeners when disposed", async () => {
  const audio = await load();
  const dispose = audio.subscribeVoiceAudioStorage();
  localStorage.setItem(
    audio.VOICE_AUDIO_STORAGE_KEY,
    JSON.stringify({ inputDeviceId: "other-tab", outputDeviceId: "" }),
  );
  window.dispatchEvent(
    new StorageEvent("storage", {
      key: audio.VOICE_AUDIO_STORAGE_KEY,
      storageArea: localStorage,
      newValue: localStorage.getItem(audio.VOICE_AUDIO_STORAGE_KEY),
    }),
  );
  expect(audio.useVoiceAudioStore.getState().preferences.inputDeviceId).toBe("other-tab");
  window.dispatchEvent(
    new StorageEvent("storage", {
      key: audio.VOICE_AUDIO_STORAGE_KEY,
      storageArea: localStorage,
      newValue: null,
    }),
  );
  expect(audio.useVoiceAudioStore.getState().preferences.inputDeviceId).toBe("");
  dispose();
  localStorage.setItem(
    audio.VOICE_AUDIO_STORAGE_KEY,
    JSON.stringify({ inputDeviceId: "after-dispose" }),
  );
  window.dispatchEvent(new Event("focus"));
  expect(audio.useVoiceAudioStore.getState().preferences.inputDeviceId).toBe("");
});

it("keeps an in-memory choice when storage writes fail instead of restoring stale saved devices", async () => {
  const audio = await load();
  audio.useVoiceAudioStore.getState().update({ inputDeviceId: "previous" });
  const storage = window.localStorage;
  const blocked = {
    getItem: storage.getItem.bind(storage),
    setItem: () => {
      throw new Error("blocked fixture");
    },
  } as Storage;
  vi.spyOn(window, "localStorage", "get").mockReturnValue(blocked);
  audio.useVoiceAudioStore.getState().update({ inputDeviceId: "current" });
  expect(audio.voiceAudioSnapshot().inputDeviceId).toBe("current");
  expect(audio.useVoiceAudioStore.getState().storageError).toBe(true);
});

it("validates stored device IDs without inventing a device or resetting the other choice", async () => {
  localStorage.setItem(
    "cccc.voice.audio.v1",
    JSON.stringify({ inputDeviceId: "x".repeat(513), outputDeviceId: "  saved-speaker  " }),
  );
  const audio = await load();
  expect(audio.voiceAudioSnapshot()).toEqual({
    inputDeviceId: "",
    outputDeviceId: "saved-speaker",
  });
  audio.useVoiceAudioStore.getState().update({ inputDeviceId: "saved-mic" });
  expect(audio.useVoiceAudioStore.getState().preferences.outputDeviceId).toBe("saved-speaker");
});
