// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vite-plus/test";
import { useCodexVoicePreferencesState } from "./useCodexVoicePreferencesState";
import { useVoiceAudioStore } from "../../stores/useVoiceAudioStore";

let host: HTMLDivElement;
let root: Root;
let current: ReturnType<typeof useCodexVoicePreferencesState>;
const storageKey = "cccc.codexVoice.preferences.v1";

function Probe() {
  current = useCodexVoicePreferencesState();
  return null;
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  localStorage.clear();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("accepts new server voices and retains a selection across remounts", async () => {
  await act(async () => root.render(<Probe />));
  await act(async () => current.acceptSupportedVoices(["cove", "himari", "aube"], "cove"));
  expect(current.supportedVoices).toEqual(["cove", "himari", "aube"]);
  await act(async () => current.updatePreferences({ voice: "himari" }));
  expect(current.preferences.voice).toBe("himari");
  expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual({
    voice: "himari",
    provider: "codex",
    grokVoice: "eve",
  });

  await act(async () => root.unmount());
  root = createRoot(host);
  await act(async () => root.render(<Probe />));
  expect(current.preferences.voice).toBe("himari");
  await act(async () => current.acceptSupportedVoices(["cove", "himari", "aube"], "cove"));
  expect(current.preferences.voice).toBe("himari");
});

it("preserves a saved new voice until its server catalog arrives", async () => {
  localStorage.setItem(storageKey, JSON.stringify({ voice: " Rindo " }));
  await act(async () => root.render(<Probe />));
  expect(current.preferences.voice).toBe("rindo");
  expect(current.supportedVoices).toEqual([]);
  await act(async () => current.acceptSupportedVoices(["cove", "rindo"], "cove"));
  expect(current.preferences.voice).toBe("rindo");
});

it("uses the server default only when the saved choice is no longer supported", async () => {
  await act(async () => root.render(<Probe />));
  await act(async () => current.acceptSupportedVoices(["aube", "himari"], "aube"));
  expect(current.preferences.voice).toBe("aube");
  await act(async () => current.updatePreferences({ voice: "himari" }));
  await act(async () => current.acceptSupportedVoices(["aube", "himari"], "aube"));
  expect(current.preferences.voice).toBe("himari");
  await act(async () => current.acceptSupportedVoices(["cove", "aube"], "aube"));
  expect(current.preferences.voice).toBe("aube");
  expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual({
    voice: "aube",
    provider: "codex",
    grokVoice: "eve",
  });
});

it("keeps the last catalog and shared devices through an unavailable catalog", async () => {
  await act(async () => root.render(<Probe />));
  await act(async () => {
    current.updatePreferences({ inputDeviceId: "fixture-mic", outputDeviceId: "fixture-speaker" });
    current.acceptSupportedVoices(["cove", "himari"], "cove");
  });
  await act(async () => current.updatePreferences({ voice: "himari" }));
  await act(async () => {
    current.acceptSupportedVoices(null, "cove");
    current.acceptSupportedVoices([], "cove");
  });
  expect(current.supportedVoices).toEqual(["cove", "himari"]);
  expect(current.preferences).toEqual({
    provider: "codex",
    voice: "himari",
    inputDeviceId: "fixture-mic",
    outputDeviceId: "fixture-speaker",
  });
  expect(useVoiceAudioStore.getState().preferences.inputDeviceId).toBe("fixture-mic");
  expect(JSON.parse(localStorage.getItem(storageKey)!)).toEqual({
    voice: "himari",
    provider: "codex",
    grokVoice: "eve",
  });
});

it("remembers each service's voice independently while sharing microphone and speaker", async () => {
  await act(async () => root.render(<Probe />));
  await act(async () => {
    current.acceptSupportedVoices(["cove", "himari"], "cove", "codex");
    current.acceptSupportedVoices(["eve", "ara", "rex"], "eve", "grok");
    current.updatePreferences({ voice: "himari", inputDeviceId: "shared-mic" });
  });
  await act(async () => current.updatePreferences({ provider: "grok" }));
  expect(current.preferences.voice).toBe("eve");
  expect(current.preferences.inputDeviceId).toBe("shared-mic");
  expect(current.supportedVoices).toEqual(["eve", "ara", "rex"]);
  await act(async () => current.updatePreferences({ voice: "ara" }));
  await act(async () => current.updatePreferences({ provider: "codex" }));
  expect(current.preferences.voice).toBe("himari");
  await act(async () => root.unmount());
  root = createRoot(host);
  await act(async () => root.render(<Probe />));
  await act(async () => current.updatePreferences({ provider: "grok" }));
  expect(current.preferences.voice).toBe("ara");
});
