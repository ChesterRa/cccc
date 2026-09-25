// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useModalStore } from "../../stores";
import type { ActorSecretChanges } from "./actorSecretManagerModel";
const mocks = vi.hoisted(() => ({ env: vi.fn(), status: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.status(...a),
  fetchActorPrivateEnvKeys: (...a: unknown[]) => mocks.env(...a),
}));
vi.mock("../CapabilityPicker", () => ({ CapabilityPicker: () => <div>capability-picker</div> }));
vi.mock("../RolePresetPicker", () => ({ RolePresetPicker: () => <div>actor-preset</div> }));
vi.mock("../webModel/WebModelActorSetup", () => ({
  WebModelActorSetup: ({ onOpenSharedSettings }: { onOpenSharedSettings?: () => void }) => (
    <div>
      conversation-setup<button onClick={onOpenSharedSettings}>shared-settings</button>
    </div>
  ),
}));
vi.mock("./ModalFrame", () => ({
  ModalFrame: ({
    children,
    footerActions,
    isOpen,
  }: {
    children: ReactNode;
    footerActions: ReactNode;
    isOpen: boolean;
  }) => (
    <div data-visible={isOpen}>
      {children}
      {footerActions}
    </div>
  ),
}));
vi.mock("./ActorSecretManager", () => ({
  ActorSecretManager: ({
    changes,
    onChangesChange,
  }: {
    changes: ActorSecretChanges;
    onChangesChange: (c: ActorSecretChanges) => void;
  }) => (
    <button
      onClick={() =>
        onChangesChange({
          ...changes,
          setVars: { FIXTURE_KEY: "synthetic" },
          unsetKeys: ["OLD_FIXTURE"],
          clearAll: false,
        })
      }
    >
      stage-env-draft
    </button>
  ),
}));
import { ActorConfigModal, type EditActorConfigProps } from "./ActorConfigModal";

describe("Web Model effective Actor configuration", () => {
  const host = document.createElement("div");
  let root: ReturnType<typeof createRoot>;
  const props = (): EditActorConfigProps => ({
    mode: "edit",
    isOpen: true,
    isDark: false,
    busy: "",
    groupId: "g_fixture",
    actorId: "alpha",
    isRunning: false,
    savedRuntime: "codex",
    runtimes: [],
    runtime: "codex",
    onChangeRuntime: vi.fn(),
    command: "codex",
    onChangeCommand: vi.fn(),
    title: "Alpha",
    onChangeTitle: vi.fn(),
    actorNotes: "Keep these instructions",
    onChangeActorNotes: vi.fn(),
    capabilityAutoloadText: "skill:fixture",
    onChangeCapabilityAutoloadText: vi.fn(),
    actorProfiles: [],
    actorProfilesBusy: false,
    onSaveAsProfile: vi.fn(),
    onSave: vi.fn().mockResolvedValue(undefined),
    onSaveAndRestart: vi.fn().mockResolvedValue(undefined),
    onCancel: vi.fn(),
  });
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.clearAllMocks();
    useModalStore.getState().closeModal("settings");
    document.body.append(host);
    root = createRoot(host);
    mocks.env.mockResolvedValue({ ok: true, result: { keys: ["OLD_FIXTURE"] } });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });
  const button = (label: string) =>
    [...host.querySelectorAll("button")].find((b) => b.textContent === label)!;
  it("hides launch fields without fetching secrets while preserving valid controls and saving", async () => {
    const p = props();
    p.runtime = "web_model";
    p.savedRuntime = "web_model";
    p.command = "";
    await act(async () => root.render(<ActorConfigModal {...p} />));
    expect(mocks.env).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain("stage-env-draft");
    expect(button("secretsSection")).toBeUndefined();
    expect(host.textContent).toContain("capability-picker");
    expect(host.textContent).toContain("actor-preset");
    expect(host.textContent).toContain("webModelProfileHint");
    expect(host.textContent).toContain("conversation-setup");
    expect(host.querySelector('input[placeholder="enterCommand"]')).toBeNull();
    expect(button("common:done").disabled).toBe(false);
    await act(async () => button("common:done").click());
    expect(p.onCancel).toHaveBeenCalledOnce();
    expect(p.onSave).not.toHaveBeenCalled();
    await act(async () => root.render(<ActorConfigModal {...p} title="Changed" />));
    await act(async () => button("common:save").click());
    expect(p.onSave).toHaveBeenCalledWith(
      expect.objectContaining({
        setVars: {},
        unsetKeys: [],
        clear: false,
        capabilityAutoload: ["skill:fixture"],
      }),
    );
  });
  it("does not apply hidden launch drafts and restores them if the user switches back", async () => {
    const p = props();
    await act(async () => root.render(<ActorConfigModal {...p} />));
    await act(async () => button("stage-env-draft").click());
    await act(async () => root.render(<ActorConfigModal {...p} runtime="web_model" command="" />));
    expect(host.textContent).not.toContain("conversation-setup");
    await act(async () => button("common:save").click());
    expect(p.onSave).toHaveBeenLastCalledWith(
      expect.objectContaining({ setVars: {}, unsetKeys: [], clear: false }),
    );
    await act(async () => root.render(<ActorConfigModal {...p} />));
    await act(async () => button("common:save").click());
    expect(p.onSave).toHaveBeenLastCalledWith(
      expect.objectContaining({
        setVars: { FIXTURE_KEY: "synthetic" },
        unsetKeys: ["OLD_FIXTURE"],
        clear: false,
      }),
    );
  });
  it("opening shared settings suspends the Actor dialog without discarding its draft", async () => {
    const p = props();
    p.runtime = "web_model";
    p.savedRuntime = "web_model";
    await act(async () => root.render(<ActorConfigModal {...p} />));
    await act(async () => button("fromActorProfile").click());
    await act(async () => button("shared-settings").click());
    expect(useModalStore.getState().settingsTarget).toMatchObject({
      scope: "global",
      tab: "webModels",
    });
    expect(host.querySelector("[data-visible]")?.getAttribute("data-visible")).toBe("false");
    expect(p.onCancel).not.toHaveBeenCalled();
    await act(async () => useModalStore.getState().closeModal("settings"));
    expect(host.querySelector("[data-visible]")?.getAttribute("data-visible")).toBe("true");
    expect(host.textContent).toContain("selectActorProfile");
  });

  it("edits a running Grok route locally and submits once from the footer", async () => {
    const p = props();
    p.runtime = "grok_web_model";
    p.savedRuntime = "grok_web_model";
    p.isRunning = true;
    const oldUrl = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
    const url = "https://grok.com/bot/958f2446-013f-4225-8dd3-f146295992d7";
    mocks.status.mockResolvedValue({
      ok: true,
      result: {
        pairing: { state: "bound", url: oldUrl, actor_enabled: true },
        browser_session: {},
      },
    });
    await act(async () => root.render(<ActorConfigModal {...p} />));
    expect(button("common:done")).toBeDefined();
    await act(async () => button("grokActor.change").click());
    const input = host.querySelector<HTMLInputElement>('input[type="url"]')!;
    expect(input.disabled).toBe(false);
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, url);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(p.onSave).not.toHaveBeenCalled();
    expect(p.onSaveAndRestart).not.toHaveBeenCalled();
    expect(button("grokActor.save")).toBeUndefined();
    expect(button("saveAndRestart")).toBeUndefined();
    p.onSaveAndRestart = vi.fn().mockRejectedValueOnce(new Error("delivery_unresolved"));
    await act(async () => root.render(<ActorConfigModal {...p} />));
    await act(async () => button("saveAndApply").click());
    expect(p.onSaveAndRestart).toHaveBeenCalledWith(expect.objectContaining({ grokBotUrl: url }));
    expect(input.value).toBe(url);
    expect(host.textContent).toContain("delivery_unresolved");
  });

  it("saves an Actor-local Bot URL while retaining its linked Grok Profile", async () => {
    const p = props();
    p.runtime = "grok_web_model";
    p.savedRuntime = "grok_web_model";
    p.linkedProfileId = "grok-profile";
    p.actorProfiles = [
      {
        id: "grok-profile",
        name: "Grok",
        runtime: "grok_web_model",
        scope: "global",
        runner: "headless",
        command: [],
        submit: "enter",
        env: {},
        created_at: "",
        updated_at: "",
        revision: 1,
      },
    ];
    mocks.status.mockResolvedValue({
      ok: true,
      result: { pairing: { state: "unpaired", actor_enabled: false }, browser_session: {} },
    });
    await act(async () => root.render(<ActorConfigModal {...p} />));
    const input = host.querySelector<HTMLInputElement>('input[type="url"]')!;
    const url = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, url);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(button("common:save")).toBeDefined();
    await act(async () => button("common:save").click());
    expect(p.onSave).toHaveBeenCalledWith(
      expect.objectContaining({ mode: "profile", grokBotUrl: url }),
    );
  });

  it("compares a failed multi-step save against the persisted Actor while keeping the draft", async () => {
    const p = props();
    p.savedActor = {
      id: p.actorId,
      runtime: "codex",
      command: ["codex"],
      title: p.title,
      capability_autoload: ["skill:fixture"],
    };
    await act(async () => root.render(<ActorConfigModal {...p} />));
    await act(async () =>
      root.render(<ActorConfigModal {...p} runtime="grok_web_model" title="Saved title" />),
    );
    const input = host.querySelector<HTMLInputElement>('input[type="url"]')!;
    const url = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, url);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    p.onSave = vi.fn().mockRejectedValue(new Error("connector missing"));
    mocks.status.mockResolvedValue({
      ok: true,
      result: { pairing: { state: "unpaired", actor_enabled: false }, browser_session: {} },
    });
    p.savedRuntime = "grok_web_model";
    p.savedActor = { ...p.savedActor, runtime: "grok_web_model", title: "Saved title" };
    await act(async () =>
      root.render(<ActorConfigModal {...p} runtime="grok_web_model" title="Saved title" />),
    );
    await act(async () => button("common:save").click());
    expect(host.querySelector<HTMLInputElement>('input[type="url"]')!.value).toBe(url);
    // Restore the old runtime/title after the server saved the new configuration.
    await act(async () => root.render(<ActorConfigModal {...p} />));
    expect(button("common:save")).toBeDefined();
    await act(async () => button("common:save").click());
    expect(p.onSave).toHaveBeenLastCalledWith(
      expect.not.objectContaining({ grokBotUrl: expect.anything() }),
    );
  });

  it("accepts a Bot URL immediately when switching to Grok and submits it with the runtime draft", async () => {
    const p = props();
    await act(async () => root.render(<ActorConfigModal {...p} />));
    await act(async () => root.render(<ActorConfigModal {...p} runtime="grok_web_model" />));
    const input = host.querySelector<HTMLInputElement>('input[type="url"]');
    expect(input).not.toBeNull();
    expect(input!.disabled).toBe(false);
    expect(button("common:save").disabled).toBe(true);
    const fill = async (value: string) => {
      await act(async () => {
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
          input,
          value,
        );
        input!.dispatchEvent(new Event("input", { bubbles: true }));
      });
    };
    await fill("https://grok.com/");
    await act(async () => button("common:save").click());
    expect(p.onSave).not.toHaveBeenCalled();
    expect(host.textContent).toContain("settings:grokActor.invalidUrl");
    const url = "https://grok.com/bot/1373170d-9cf2-408c-b597-e243e5884f4a";
    await fill(url);
    p.onSave = vi
      .fn()
      .mockRejectedValueOnce(new Error("fixture bind failed"))
      .mockResolvedValue(undefined);
    await act(async () => root.render(<ActorConfigModal {...p} runtime="grok_web_model" />));
    await act(async () => button("common:save").click());
    expect(p.onSave).toHaveBeenCalledWith(expect.objectContaining({ grokBotUrl: url }));
    expect(input!.value).toBe(url);
    expect(host.textContent).toContain("fixture bind failed");
    await act(async () => button("saveAndRestart").click());
    expect(p.onSaveAndRestart).toHaveBeenCalledWith(expect.objectContaining({ grokBotUrl: url }));
    await act(async () => root.render(<ActorConfigModal {...p} />));
    expect(host.querySelector('input[type="url"]')).toBeNull();
    const calls = vi.mocked(p.onSave).mock.calls.length;
    await act(async () => button("common:done").click());
    expect(p.onSave).toHaveBeenCalledTimes(calls);
    await act(async () => root.render(<ActorConfigModal {...p} runtime="grok_web_model" />));
    expect(host.querySelector<HTMLInputElement>('input[type="url"]')!.value).toBe(url);
    await act(async () =>
      root.render(<ActorConfigModal {...p} actorId="beta" runtime="grok_web_model" />),
    );
    expect(host.querySelector<HTMLInputElement>('input[type="url"]')!.value).toBe("");
  });
});
