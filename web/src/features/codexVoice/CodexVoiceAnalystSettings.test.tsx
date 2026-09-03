// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { CodexVoiceAnalystSettings } from "./CodexVoiceAnalystSettings";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

const api = vi.hoisted(() => ({
  fetchSettings: vi.fn(),
  listProfiles: vi.fn(),
  updateSettings: vi.fn(),
  upsertProfile: vi.fn(),
  copyVoiceSecrets: vi.fn(),
  updateProfileEnv: vi.fn(),
}));

vi.mock("react-i18next", () => {
  const t = (key: string) => key;
  return { useTranslation: () => ({ t }) };
});
vi.mock("../../services/api", () => ({
  fetchCodexVoiceAnalystSettings: api.fetchSettings,
  listActorProfiles: api.listProfiles,
  updateCodexVoiceAnalystSettings: api.updateSettings,
  upsertActorProfile: api.upsertProfile,
  copyVoiceAnalystPrivateEnvToProfile: api.copyVoiceSecrets,
  updateProfilePrivateEnv: api.updateProfileEnv,
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const customSettings = {
  runtime: "codex",
  command: [],
  profile_id: "",
  profile_scope: "global" as const,
  profile_owner: "",
};

afterEach(() => {
  api.fetchSettings.mockReset();
  api.listProfiles.mockReset();
  api.updateSettings.mockReset();
  api.upsertProfile.mockReset();
  api.copyVoiceSecrets.mockReset();
  api.updateProfileEnv.mockReset();
  vi.restoreAllMocks();
  Reflect.deleteProperty(window, "prompt");
  document.body.innerHTML = "";
});

function controller(): CodexVoiceSessionController {
  return {
    isEngaged: false,
    analyst: null,
    readiness: {
      analyst_runtime: "codex",
      analyst_runtime_available: true,
      realtime_credentials_available: true,
    },
    refresh: vi.fn(async () => undefined),
  } as unknown as CodexVoiceSessionController;
}

function buttonWithText(container: HTMLElement, text: string): HTMLButtonElement {
  const button = [...container.querySelectorAll("button")].find(
    (candidate) => candidate.textContent?.trim() === text,
  );
  if (!(button instanceof HTMLButtonElement)) throw new Error(`button not found: ${text}`);
  return button;
}

async function renderSettings() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () =>
    root.render(<CodexVoiceAnalystSettings active controller={controller()} />),
  );
  await act(async () => undefined);
  return { host, root };
}

describe("CodexVoiceAnalystSettings", () => {
  it("uses the same runtime mode and private environment controls as Actor editing", async () => {
    api.fetchSettings.mockResolvedValue({
      ok: true,
      result: { settings: customSettings, environment_keys: ["OPENAI_API_KEY"] },
    });
    api.listProfiles.mockResolvedValue({ ok: true, result: { profiles: [] } });

    const { host, root } = await renderSettings();

    expect(host.textContent).toContain("creationMode");
    expect(host.textContent).toContain("customAgent");
    expect(host.textContent).toContain("fromActorProfile");
    expect(host.textContent).toContain("useRuntimeDefaultCommand");
    expect(host.textContent).toContain("secretManager.addVariable");
    expect(host.textContent).toContain("secretManager.batchPaste");
    expect(host.querySelector("details")?.open).toBe(false);
    await act(async () => buttonWithText(host, "fromActorProfile").click());
    expect(host.textContent).toContain("codexVoiceAnalystCompatibleProfilesEmpty");
    expect(host.textContent).not.toContain("secretManager.addVariable");
    await act(async () => root.unmount());
  });

  it("lets the user replace the runtime default with an editable explicit command", async () => {
    api.fetchSettings.mockResolvedValue({
      ok: true,
      result: { settings: customSettings, environment_keys: [] },
    });
    api.listProfiles.mockResolvedValue({ ok: true, result: { profiles: [] } });
    api.updateSettings.mockResolvedValue({
      ok: true,
      result: { analyst: null, restarted: false, started_new_session: false },
    });

    const { host, root } = await renderSettings();
    const toggle = host.querySelector('input[type="checkbox"]');
    if (!(toggle instanceof HTMLInputElement)) throw new Error("default command toggle not found");
    expect(toggle.checked).toBe(true);

    await act(async () => toggle.click());

    expect(toggle.checked).toBe(false);
    const command = host.querySelector('input[placeholder="codex"]');
    expect(command).toBeInstanceOf(HTMLInputElement);
    expect((command as HTMLInputElement).value).toBe("codex");
    await act(async () => buttonWithText(host, "codexVoiceAnalystSettingsSave").click());
    expect(api.updateSettings).toHaveBeenCalledWith({
      settings: { ...customSettings, command: "codex" },
      environmentSet: {},
      environmentUnset: [],
      environmentClear: false,
    });
    await act(async () => root.unmount());
  });

  it("sends the shared clear-all draft as one atomic Voice settings update", async () => {
    api.fetchSettings.mockResolvedValue({
      ok: true,
      result: { settings: customSettings, environment_keys: ["OPENAI_API_KEY"] },
    });
    api.listProfiles.mockResolvedValue({ ok: true, result: { profiles: [] } });
    api.updateSettings.mockResolvedValue({
      ok: true,
      result: { analyst: null, restarted: false, started_new_session: false },
    });
    const { host, root } = await renderSettings();

    await act(async () => buttonWithText(host, "secretManager.clearAllAction").click());
    await act(async () => buttonWithText(host, "codexVoiceAnalystSettingsSave").click());

    expect(api.updateSettings).toHaveBeenCalledWith({
      settings: { ...customSettings, command: "" },
      environmentSet: {},
      environmentUnset: [],
      environmentClear: true,
    });
    await act(async () => root.unmount());
  });

  it("binds a Codex Runtime Profile without copying its command or secrets", async () => {
    api.fetchSettings.mockResolvedValue({
      ok: true,
      result: { settings: customSettings, environment_keys: ["CUSTOM_SECRET"] },
    });
    api.listProfiles.mockResolvedValue({
      ok: true,
      result: {
        profiles: [
          {
            id: "voice-codex",
            name: "Voice Codex",
            scope: "global",
            owner_id: "",
            runtime: "codex",
            runner: "pty",
            command: "codex --profile voice",
            submit: "enter",
            env: {},
            created_at: "2026-09-01T00:00:00Z",
            updated_at: "2026-09-01T00:00:00Z",
            revision: 1,
          },
        ],
      },
    });
    api.updateSettings.mockResolvedValue({
      ok: true,
      result: { analyst: null, restarted: false, started_new_session: false },
    });
    const { host, root } = await renderSettings();

    await act(async () => buttonWithText(host, "fromActorProfile").click());
    expect(host.textContent).toContain("Voice Codex");
    expect(host.textContent).toContain("codex --profile voice");
    expect(host.textContent).toContain("codexVoiceAnalystProfileDetails");
    expect(host.querySelector("details")?.open).toBe(false);
    expect(host.textContent).not.toContain("secretManager.addVariable");
    await act(async () => buttonWithText(host, "codexVoiceAnalystSettingsSave").click());

    expect(api.updateSettings).toHaveBeenCalledWith({
      settings: { ...customSettings, command: "", profile_id: "voice-codex" },
      environmentSet: {},
      environmentUnset: [],
      environmentClear: false,
    });
    await act(async () => root.unmount());
  });

  it("saves custom Analyst configuration and write-only secrets as a Runtime Profile", async () => {
    api.fetchSettings.mockResolvedValue({
      ok: true,
      result: { settings: customSettings, environment_keys: ["ZAI_API_KEY"] },
    });
    api.listProfiles.mockResolvedValue({ ok: true, result: { profiles: [] } });
    const profile = {
      id: "voice-zai",
      name: "Voice ZAI",
      scope: "global",
      owner_id: "",
      runtime: "codex",
      runner: "pty",
      command: [],
      submit: "enter",
      env: {},
      created_at: "2026-09-02T00:00:00Z",
      updated_at: "2026-09-02T00:00:00Z",
      revision: 1,
    };
    api.upsertProfile.mockResolvedValue({ ok: true, result: { profile } });
    api.copyVoiceSecrets.mockResolvedValue({
      ok: true,
      result: { profile_id: profile.id, keys: ["ZAI_API_KEY"] },
    });
    api.updateProfileEnv.mockResolvedValue({
      ok: true,
      result: { profile_id: profile.id, keys: [] },
    });
    api.updateSettings.mockResolvedValue({
      ok: true,
      result: { analyst: null, restarted: false, started_new_session: false },
    });
    Object.defineProperty(window, "prompt", {
      configurable: true,
      value: vi.fn().mockReturnValue("Voice ZAI"),
    });
    const { host, root } = await renderSettings();

    await act(async () => buttonWithText(host, "secretManager.clearAllAction").click());
    await act(async () => buttonWithText(host, "addToActorProfiles").click());

    expect(api.upsertProfile).toHaveBeenCalledWith({
      name: "Voice ZAI",
      runtime: "codex",
      runner: "pty",
      command: "",
      submit: "enter",
      env: {},
    });
    expect(api.copyVoiceSecrets).toHaveBeenCalledWith("voice-zai");
    expect(api.updateProfileEnv).toHaveBeenCalledWith("voice-zai", {}, [], true, {
      scope: "global",
      ownerId: "",
    });
    expect(host.textContent).toContain("codexVoiceAnalystProfileCreated");

    await act(async () => buttonWithText(host, "codexVoiceAnalystSettingsSave").click());
    expect(api.updateSettings).toHaveBeenCalledWith({
      settings: { ...customSettings, command: "", profile_id: "voice-zai" },
      environmentSet: {},
      environmentUnset: [],
      environmentClear: false,
    });
    await act(async () => root.unmount());
  });

  it("recovers the full settings form when a failed initial load is refreshed", async () => {
    api.fetchSettings
      .mockResolvedValueOnce({
        ok: false,
        error: { code: "codex_voice_settings_unavailable", message: "load failed" },
      })
      .mockResolvedValueOnce({
        ok: true,
        result: { settings: customSettings, environment_keys: ["OPENAI_API_KEY"] },
      });
    api.listProfiles.mockResolvedValue({ ok: true, result: { profiles: [] } });
    const { host, root } = await renderSettings();

    expect(host.textContent).toContain("codexVoiceAnalystSettingsUnavailable");
    const refresh = host.querySelector('button[aria-label="refreshConfiguredKeys"]');
    if (!(refresh instanceof HTMLButtonElement)) throw new Error("refresh button not found");
    await act(async () => refresh.click());

    expect(host.textContent).not.toContain("codexVoiceAnalystSettingsUnavailable");
    expect(host.textContent).toContain("OPENAI_API_KEY");
    await act(async () => root.unmount());
  });
});
