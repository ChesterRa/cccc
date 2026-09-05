// @vitest-environment happy-dom
// Covers every non-Codex Runtime admitted to the shared Analyst settings path.

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
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

afterEach(() => {
  Object.values(api).forEach((mock) => mock.mockReset());
  vi.restoreAllMocks();
  Reflect.deleteProperty(window, "confirm");
  document.body.innerHTML = "";
});

for (const candidate of [
  {
    runtime: "kilo",
    id: "voice-kilo",
    name: "Voice Kilo",
    label: "Kilo Code CLI",
    command: "kilo --model openai/gpt-5",
  },
  {
    runtime: "claude",
    id: "voice-claude",
    name: "Voice Claude",
    label: "Claude Code",
    command: "claude --model opus",
  },
  {
    runtime: "grok",
    id: "voice-grok",
    name: "Voice Grok",
    label: "Grok Build",
    command: "grok --model grok-code-fast-1",
  },
  {
    runtime: "opencode",
    id: "voice-opencode",
    name: "Voice OpenCode",
    label: "OpenCode",
    command: "opencode --model openai/gpt-5",
  },
] as const) {
  it(`offers an admitted ${candidate.label} Runtime Profile and persists its runtime identity`, async () => {
    const settings = {
      runtime: "codex",
      command: [],
      profile_id: "",
      profile_scope: "global" as const,
      profile_owner: "",
    };
    api.fetchSettings.mockResolvedValue({ ok: true, result: { settings, environment_keys: [] } });
    api.listProfiles.mockResolvedValue({
      ok: true,
      result: {
        profiles: [
          {
            id: candidate.id,
            name: candidate.name,
            scope: "global",
            owner_id: "",
            runtime: candidate.runtime,
            runner: "pty",
            command: candidate.command,
            submit: "enter",
            env: {},
            created_at: "2026-09-02T00:00:00Z",
            updated_at: "2026-09-02T00:00:00Z",
            revision: 1,
          },
        ],
      },
    });
    api.updateSettings.mockResolvedValue({
      ok: true,
      result: { analyst: null, restarted: false, started_new_session: true },
    });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    const controller = {
      isEngaged: false,
      analyst: null,
      readiness: {
        analyst_runtime: "codex",
        analyst_runtime_available: true,
        realtime_credentials_available: true,
      },
      refresh: vi.fn(async () => undefined),
    } as unknown as CodexVoiceSessionController;
    await act(async () =>
      root.render(<CodexVoiceAnalystSettings active controller={controller} />),
    );
    await act(async () => undefined);

    const buttons = [...host.querySelectorAll("button")];
    await act(async () =>
      buttons.find((button) => button.textContent === "fromActorProfile")?.click(),
    );
    expect(host.textContent).toContain(candidate.name);
    expect(host.textContent).toContain(candidate.label);
    if (candidate.runtime === "opencode" || candidate.runtime === "kilo") {
      expect(host.textContent).toContain("opencodeManagedModelHint");
    } else {
      expect(host.textContent).not.toContain("opencodeManagedModelHint");
    }
    const confirm = vi.fn(() => true);
    Object.defineProperty(window, "confirm", { configurable: true, value: confirm });
    controller.analyst = { tui_ready: true } as never;
    await act(async () =>
      buttons.find((button) => button.textContent === "codexVoiceAnalystSettingsSave")?.click(),
    );

    expect(confirm).toHaveBeenCalledWith("codexVoiceAnalystIdentityChangeConfirm");
    expect(api.updateSettings).toHaveBeenCalledWith({
      settings: { ...settings, command: "", profile_id: candidate.id },
      environmentSet: {},
      environmentUnset: [],
      environmentClear: false,
      discardCurrentWork: false,
    });
    await act(async () => root.unmount());
  });
}
