// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
const api = vi.hoisted(() => ({
  t: (key: string) => key,
  fetch: vi.fn(async () => ({
    ok: true,
    result: { settings: { runtime: "codex", command: [], profile_id: "" }, environment_keys: [] },
  })),
  profiles: vi.fn(async () => ({ ok: true, result: { profiles: [] } })),
}));
const controller = vi.hoisted(() => ({ analyst: null, isEngaged: false, start: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: api.t }) }));
vi.mock("../../services/api", () => ({
  fetchCodexVoiceAnalystSettings: api.fetch,
  listActorProfiles: api.profiles,
}));
vi.mock("./useCodexVoiceSessionController", () => ({
  useCodexVoiceSessionController: () => controller,
}));
import { useCodexVoiceShell } from "./useCodexVoiceShell";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => {
  document.body.innerHTML = "";
  vi.clearAllMocks();
});

describe("shared voice settings draft lifetime", () => {
  it("keeps the Analyst command and private-key draft when the settings surface closes", async () => {
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let voice!: ReturnType<typeof useCodexVoiceShell>;
    function Harness() {
      voice = useCodexVoiceShell(true);
      return <div>{voice.analystSettings.settings.command}</div>;
    }
    try {
      await act(async () => root.render(<Harness />));
      expect(api.fetch).not.toHaveBeenCalled();
      await act(async () => voice.setAnalystSettingsActive(true));
      expect(api.fetch).toHaveBeenCalledTimes(1);
      await act(async () => {
        voice.analystSettings.setCommand("codex --model fixture-model");
        voice.analystSettings.setEnvironmentChanges({
          ...voice.analystSettings.environmentChanges,
          setVars: { FIXTURE_PRIVATE: "synthetic-draft" },
        });
      });
      const privateDraft = voice.analystSettings.environmentChanges;
      await act(async () => voice.setAnalystSettingsActive(false));
      await act(async () => root.render(<Harness />));
      await act(async () => voice.setAnalystSettingsActive(true));
      expect(voice.analystSettings.settings.command).toBe("codex --model fixture-model");
      expect(voice.analystSettings.environmentChanges).toEqual(privateDraft);
      expect(voice.analystSettings.hasChanges).toBe(true);
      expect(api.fetch).toHaveBeenCalledTimes(1);
      expect(controller.start).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
