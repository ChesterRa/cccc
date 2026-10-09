// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { GlobalVoiceSecretaryState } from "../../../services/api";
import { actorProfileIdentityKey } from "../../../utils/actorProfiles";
const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  save: vi.fn(),
  preferences: vi.fn(),
  upsert: vi.fn(),
  copySecrets: vi.fn(),
  updateProfileEnv: vi.fn(),
  t: (key: string) => key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
vi.mock("../../../services/api", () => ({
  fetchGlobalVoiceSecretary: mocks.fetch,
  saveGlobalVoiceSecretary: mocks.save,
  saveGlobalVoiceSecretaryPreferences: mocks.preferences,
  upsertActorProfile: mocks.upsert,
  copyVoiceSecretaryPrivateEnvToProfile: mocks.copySecrets,
  updateProfilePrivateEnv: mocks.updateProfileEnv,
}));
vi.mock("../../../features/codexVoice/CodexVoiceSettingsPanel", () => ({
  CodexVoiceSettingsPanel: () => <div data-analyst-settings />,
}));
vi.mock("./LocalAsrModels", () => ({ LocalAsrModels: () => null }));
vi.mock("../ActorSecretManager", () => ({
  ActorSecretManager: ({
    changes,
    onChangesChange,
  }: {
    changes: unknown;
    onChangesChange: (value: unknown) => void;
  }) => (
    <button
      type="button"
      onClick={() =>
        onChangesChange({
          setVars: { SECRETARY_FIXTURE: "fixture-draft" },
          unsetKeys: [],
          clearAll: false,
        })
      }
      data-staged-secret={JSON.stringify(changes)}
    >
      stage-secret
    </button>
  ),
}));
vi.mock("../../../services/api/voiceAsrProviders", () => ({
  fetchVoiceAsrProviders: async () => ({
    ok: true,
    result: {
      providers: [
        {
          provider: "bailian",
          configured: false,
          region: "beijing",
          workspace_id: "",
          model: "fun-asr-realtime",
          resource_id: "",
          auth_mode: "api_key",
          has_api_key: false,
          has_app_id: false,
          has_access_token: false,
        },
      ],
    },
  }),
  saveVoiceAsrProvider: vi.fn(),
  probeVoiceAsrProvider: vi.fn(),
}));
vi.mock("../../SelectCombobox", () => ({
  SelectCombobox: ({
    value,
    items,
    onChange,
    disabled,
    ariaLabel,
  }: {
    value: string;
    items: { value: string; label: string }[];
    onChange: (value: string) => void;
    disabled: boolean;
    ariaLabel: string;
  }) => (
    <select
      aria-label={ariaLabel}
      value={value}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value)}
    >
      {items.map((item) => (
        <option key={item.value} value={item.value}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));
import { VoiceSettingsTab } from "./VoiceSettingsTab";
import { defaultSecretaryPreferences } from "./useVoiceSecretarySettings";
const state: GlobalVoiceSecretaryState = {
  settings: { profile_id: "", config: defaultSecretaryPreferences },
  environment_keys: [],
  configured: false,
  profiles: [
    {
      id: "p",
      name: "Codex",
      runtime: "codex",
      scope: "global",
      owner_id: "",
    } as GlobalVoiceSecretaryState["profiles"][number],
  ],
};
const voice = {
  analystSettings: { saving: false, profileSaving: false },
  controller: {},
  setAnalystSettingsActive: () => undefined,
} as unknown as import("../../../features/codexVoice/useCodexVoiceShell").CodexVoiceShellState;
describe("global voice configuration", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let guard: () => boolean;
  const register = (value: () => boolean) => {
    guard = value;
  };
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    mocks.fetch.mockReset().mockResolvedValue({ ok: true, result: state });
    mocks.save.mockReset();
    mocks.upsert.mockReset();
    mocks.copySecrets.mockReset();
    mocks.updateProfileEnv.mockReset();
    vi.stubGlobal(
      "prompt",
      vi.fn(() => "Secretary reusable"),
    );
    mocks.preferences
      .mockReset()
      .mockImplementation(async (patch) => ({
        ok: true,
        result: {
          ...state,
          settings: { ...state.settings, config: { ...state.settings.config, ...patch } },
        },
      }));
    guard = () => true;
    vi.stubGlobal(
      "confirm",
      vi.fn(() => false),
    );
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  const render = (section: "secretary" | "realtime" = "secretary") =>
    act(async () =>
      root.render(
        <VoiceSettingsTab
          voice={voice}
          section={section}
          onSectionChange={() => undefined}
          isDark={false}
          isActive
          onBeforeLeaveChange={register}
        />,
      ),
    );
  const button = (text: string) =>
    Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find(
      (item) => item.textContent === text,
    )!;
  const select = (label: string) =>
    host.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!;
  const change = async (label: string, value: string) =>
    act(async () => {
      const input = select(label);
      input.value = value;
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
  it("offers two feature tabs and keeps Secretary and Analyst drafts on the same surface", async () => {
    await render();
    expect(host.querySelectorAll('[role="tab"]').length).toBe(2);
    expect(select("runtime").value).toBe("");
    await change("runtime", "claude");
    await render("realtime");
    await render();
    expect(select("runtime").value).toBe("claude");
    expect(guard()).toBe(false);
  });
  it("preserves a Profile draft across refresh and saves its scoped identity", async () => {
    let resolve!: (value: unknown) => void;
    mocks.fetch.mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
    await render();
    expect(button("common:save").disabled).toBe(true);
    await act(async () => resolve({ ok: true, result: state }));
    await act(async () => button("fromActorProfile").click());
    expect(select("actorProfile").value).toBe(actorProfileIdentityKey(state.profiles[0]));
    expect(guard()).toBe(false);
    await act(async () => button("voiceSettings.refresh").click());
    expect(select("actorProfile").value).toBe(actorProfileIdentityKey(state.profiles[0]));
    mocks.save.mockResolvedValueOnce({
      ok: true,
      result: { ...state, configured: true, settings: { ...state.settings, profile_id: "p" } },
    });
    await act(async () => button("common:save").click());
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({ profile_id: "p", profile_scope: "global" }),
      {},
    );
    expect(guard()).toBe(true);
  });
  it("preserves staged secrets and runtime on failure, then clears only after success", async () => {
    await render();
    await change("runtime", "copilot");
    await act(async () => button("stage-secret").click());
    mocks.save.mockResolvedValueOnce({
      ok: false,
      error: { code: "fixture_failure", message: "save failed" },
    });
    await act(async () => button("common:save").click());
    expect(button("common:save").disabled).toBe(false);
    expect(button("stage-secret").dataset.stagedSecret).toContain("fixture-draft");
    mocks.save.mockResolvedValueOnce({
      ok: true,
      result: {
        ...state,
        configured: true,
        environment_keys: ["SECRETARY_FIXTURE"],
        settings: { ...state.settings, runtime: "copilot", runtime_mode: "acp" },
      },
    });
    await act(async () => button("common:save").click());
    expect(mocks.save).toHaveBeenLastCalledWith(
      expect.objectContaining({ runtime: "copilot", runtime_mode: "acp" }),
      expect.objectContaining({
        environment: { set: { SECRETARY_FIXTURE: "fixture-draft" }, unset: [], clear: false },
      }),
    );
    expect(button("common:save").disabled).toBe(true);
    expect(button("stage-secret").dataset.stagedSecret).not.toContain("fixture-draft");
  });
  it("autosaves recognition defaults independently of Runtime and credential drafts", async () => {
    await render();
    await change("runtime", "claude");
    await act(async () => button("stage-secret").click());
    await change("voiceSettings.defaultLanguage", "ja-JP");
    expect(mocks.preferences).toHaveBeenCalledExactlyOnceWith({ recognition_language: "ja-JP" });
    expect(mocks.save).not.toHaveBeenCalled();
    expect(select("runtime").value).toBe("claude");
    expect(button("stage-secret").dataset.stagedSecret).toContain("fixture-draft");
    expect(button("common:save").disabled).toBe(false);
    expect(guard()).toBe(false);
  });
  it("retains failed preference drafts and retries without submitting the Runtime", async () => {
    await render();
    await change("runtime", "cursor");
    mocks.preferences.mockResolvedValueOnce({
      ok: false,
      error: { code: "fixture", message: "preference failure" },
    });
    await change("voiceSettings.defaultLanguage", "ja-JP");
    expect(select("voiceSettings.defaultLanguage").value).toBe("ja-JP");
    expect(host.querySelector('#voice-settings-secretary-panel [role="alert"]')?.textContent).toBe(
      "preference failure",
    );
    await act(async () => button("voiceSettings.retryPreferences").click());
    expect(mocks.preferences).toHaveBeenLastCalledWith({ recognition_language: "ja-JP" });
    expect(select("runtime").value).toBe("cursor");
    expect(mocks.save).not.toHaveBeenCalled();
  });
  it("keeps an earlier failed preference visible after a different preference succeeds", async () => {
    await render();
    mocks.preferences.mockResolvedValueOnce({
      ok: false,
      error: { code: "fixture", message: "language not saved" },
    });
    await change("voiceSettings.defaultLanguage", "ja-JP");
    await change("assistants.recognitionBackend", "external_provider_asr");
    expect(select("voiceSettings.defaultLanguage").value).toBe("ja-JP");
    expect(host.querySelector('#voice-settings-secretary-panel [role="alert"]')?.textContent).toBe(
      "language not saved",
    );
    await act(async () => button("voiceSettings.retryPreferences").click());
    expect(mocks.preferences).toHaveBeenLastCalledWith({ recognition_language: "ja-JP" });
  });
  it("saving Runtime omits preferences and keeps unsaved work rules", async () => {
    await render();
    await change("runtime", "codex");
    const rules = host.querySelector<HTMLTextAreaElement>(
      'textarea[aria-label="voiceSettings.workRules"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!.call(
        rules,
        "unsaved-rules",
      );
      rules.dispatchEvent(new Event("input", { bubbles: true }));
    });
    mocks.save.mockResolvedValueOnce({
      ok: true,
      result: {
        ...state,
        settings: {
          ...state.settings,
          runtime: "codex",
          config: { ...defaultSecretaryPreferences, recognition_language: "ja-JP" },
        },
      },
    });
    await act(async () => button("common:save").click());
    expect(mocks.save.mock.calls[0][0]).not.toHaveProperty("config");
    expect(rules.value).toBe("unsaved-rules");
    expect(select("voiceSettings.defaultLanguage").value).toBe("ja-JP");
    expect(button("common:save").disabled).toBe(true);
    expect(button("voiceSettings.saveRules").disabled).toBe(false);
    expect(guard()).toBe(false);
    await act(async () => button("voiceSettings.saveRules").click());
    expect(mocks.preferences).toHaveBeenLastCalledWith({ guidance: "unsaved-rules" });
    expect(guard()).toBe(true);
  });
  it("copies only Secretary secrets to a new Profile and retains drafts across partial failure", async () => {
    await render();
    await change("runtime", "copilot");
    await act(async () => button("stage-secret").click());
    const profile = {
      id: "new-profile",
      name: "Secretary reusable",
      runtime: "copilot",
      runtime_mode: "acp",
      command: [],
      revision: 1,
      scope: "global",
      owner_id: "",
    };
    mocks.upsert.mockResolvedValue({ ok: true, result: { profile } });
    mocks.copySecrets.mockResolvedValue({ ok: true, result: { keys: [] } });
    mocks.updateProfileEnv.mockResolvedValueOnce({
      ok: false,
      error: { message: "fixture secret write failed" },
    });
    await act(async () => button("addToActorProfiles").click());
    expect(button("stage-secret").dataset.stagedSecret).toContain("fixture-draft");
    expect(select("runtime").value).toBe("copilot");
    expect(mocks.save).not.toHaveBeenCalled();
    mocks.updateProfileEnv.mockResolvedValueOnce({
      ok: true,
      result: { keys: ["SECRETARY_FIXTURE"] },
    });
    await act(async () => button("addToActorProfiles").click());
    expect(mocks.copySecrets).toHaveBeenCalledExactlyOnceWith("new-profile");
    expect(mocks.upsert).toHaveBeenLastCalledWith(
      expect.objectContaining({ id: "new-profile", runtime: "copilot", runtime_mode: "acp" }),
      1,
    );
    expect(mocks.updateProfileEnv).toHaveBeenLastCalledWith(
      "new-profile",
      { SECRETARY_FIXTURE: "fixture-draft" },
      [],
      false,
      { scope: "global", ownerId: "" },
    );
    expect(select("actorProfile").value).toContain("new-profile");
    expect(mocks.save).not.toHaveBeenCalled();
    expect(guard()).toBe(false);
  });
  it("saves a valid interval on blur rather than each digit or an invalid input", async () => {
    await render();
    const input = host.querySelector<HTMLInputElement>(
      'input[aria-label="assistants.documentUpdateInterval"]',
    )!;
    const fill = (value: string) =>
      act(async () => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
          input,
          value,
        );
        input.dispatchEvent(new Event("input", { bubbles: true }));
      });
    await fill("3");
    await act(async () => input.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
    expect(mocks.preferences).not.toHaveBeenCalled();
    await fill("45");
    expect(mocks.preferences).not.toHaveBeenCalled();
    await act(async () => input.dispatchEvent(new FocusEvent("focusout", { bubbles: true })));
    expect(mocks.preferences).toHaveBeenCalledExactlyOnceWith({
      auto_document_max_window_seconds: 45,
    });
  });
  it("retains unsaved ASR credentials when another section is shown", async () => {
    await render();
    await change("assistants.recognitionBackend", "external_provider_asr");
    const input = host.querySelector<HTMLInputElement>('input[type="password"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
        input,
        "fixture-private-draft",
      );
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await render("realtime");
    expect(guard()).toBe(false);
    expect(input.value).toBe("fixture-private-draft");
  });
  it("requires an explicit backlog decision on first runtime configuration", async () => {
    mocks.fetch.mockResolvedValueOnce({ ok: true, result: { ...state, backlog_sources: 3 } });
    await render();
    await change("runtime", "codex");
    expect(button("common:save").disabled).toBe(true);
    await act(async () =>
      host.querySelectorAll<HTMLInputElement>('input[type="radio"]')[1].click(),
    );
    expect(button("common:save").disabled).toBe(false);
    mocks.save.mockResolvedValueOnce({
      ok: true,
      result: {
        ...state,
        settings: { ...state.settings, runtime: "codex" },
        configured: true,
        backlog_sources: 3,
        held_sources: 3,
      },
    });
    await act(async () => button("common:save").click());
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({ runtime: "codex" }),
      expect.objectContaining({ backlog_action: "hold" }),
    );
  });
});
