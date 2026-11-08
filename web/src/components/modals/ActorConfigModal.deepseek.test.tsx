// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useModalStore, useUIStore } from "../../stores";
const mocks = vi.hoisted(() => ({ env: vi.fn(), status: vi.fn(), native: vi.fn() }));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("../../services/api", () => ({
  fetchGrokActorState: (...a: unknown[]) => mocks.native(...a),
  fetchWebModelBrowserSurfaceSession: (...a: unknown[]) => mocks.status(...a),
  fetchActorPrivateEnvKeys: (...a: unknown[]) => mocks.env(...a),
  fetchActorProfilePrivateEnvKeys: (...a: unknown[]) => mocks.env(...a),
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
vi.mock("../webModel/DotsActorPanel", () => ({
  DotsActorPanel: ({ onOpenSharedSettings }: { onOpenSharedSettings?: () => void }) => (
    <div>
      dots-event-setup<button onClick={onOpenSharedSettings}>dots-shared-settings</button>
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
vi.mock("./ActorSecretManager", () => ({ ActorSecretManager: () => <div>env-editor</div> }));
import { ActorConfigModal, type EditActorConfigProps } from "./ActorConfigModal";

describe("DeepSeek connection in the Agent editor", () => {
  const host = document.createElement("div");
  let root: ReturnType<typeof createRoot>;
  const props = (): EditActorConfigProps => ({
    mode: "edit",
    isOpen: true,
    isDark: false,
    busy: "",
    groupId: "g_fixture",
    actorId: "deepseek",
    isRunning: false,
    savedRuntime: "deepseek",
    runtimes: [],
    runtime: "deepseek",
    onChangeRuntime: vi.fn(),
    command: "dsh-acp-demo",
    onChangeCommand: vi.fn(),
    title: "DeepSeek",
    onChangeTitle: vi.fn(),
    actorNotes: "",
    onChangeActorNotes: vi.fn(),
    capabilityAutoloadText: "",
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
    mocks.env.mockResolvedValue({
      ok: true,
      result: { keys: ["DEEPSEEK_API_KEY", "DEEPSEEK_MODEL"] },
    });
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.unstubAllGlobals();
  });
  const buttons = (label: string) =>
    [...host.querySelectorAll("button")].filter((b) => b.textContent === label);

  it("saves a new key trimmed and a removed model through the dialog's own save", async () => {
    const previous = useUIStore.getState();
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    const p = props();
    try {
      await act(async () => root.render(<ActorConfigModal {...p} />));
      const key = host.querySelector<HTMLInputElement>('input[aria-label="deepseek.apiKey"]')!;
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      await act(async () => {
        setValue.call(key, "  sk-new-fixture\n");
        key.dispatchEvent(new Event("input", { bubbles: true }));
      });
      await act(async () => buttons("deepseek.useDefault")[1].click());
      await act(async () => buttons("saveAndRestart").at(-1)!.click());
      expect(p.onSaveAndRestart).toHaveBeenCalledOnce();
      const [payload] = vi.mocked(p.onSaveAndRestart).mock.calls[0];
      expect(payload).toEqual(
        expect.objectContaining({
          setVars: { DEEPSEEK_API_KEY: "sk-new-fixture" },
          unsetKeys: ["DEEPSEEK_MODEL"],
          clear: false,
        }),
      );
      expect(JSON.stringify(payload).match(/sk-new-fixture/g)).toHaveLength(1);
      expect(p.onSave).not.toHaveBeenCalled();
    } finally {
      useUIStore.setState(previous, true);
    }
  });
});
