// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useModalStore } from "../../stores";
import type { ActorSecretChanges } from "./actorSecretManagerModel";
const mocks = vi.hoisted(() => ({ env: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
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
    expect(button("common:save").disabled).toBe(false);
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
});
