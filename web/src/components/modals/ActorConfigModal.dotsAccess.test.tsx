// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useUIStore, useModalStore } from "../../stores";

const mocks = vi.hoisted(() => ({
  load: vi.fn(),
  update: vi.fn(),
  env: vi.fn(),
  t: (key: string) => key,
}));
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: mocks.t, i18n: { language: "en" } }),
}));
vi.mock("../../services/api", () => ({
  fetchDotsActorState: (...args: unknown[]) => mocks.load(...args),
  updateDotsActorConnection: (...args: unknown[]) => mocks.update(...args),
  fetchWebModelBrowserSurfaceSession: (...args: unknown[]) => mocks.load(...args),
  fetchActorPrivateEnvKeys: mocks.env,
  fetchActorProfilePrivateEnvKeys: mocks.env,
}));
vi.mock("../CapabilityPicker", () => ({ CapabilityPicker: () => null }));
vi.mock("../RolePresetPicker", () => ({ RolePresetPicker: () => null }));
import { ActorConfigModal, type EditActorConfigProps } from "./ActorConfigModal";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const props = (): EditActorConfigProps => ({
  mode: "edit",
  isOpen: true,
  isDark: false,
  busy: "",
  groupId: "g_fixture",
  actorId: "dot",
  isRunning: true,
  savedRuntime: "chatgpt_dots",
  runtime: "chatgpt_dots",
  command: "",
  title: "Dot",
  actorNotes: "",
  capabilityAutoloadText: "",
  runtimes: [],
  actorProfiles: [],
  actorProfilesBusy: false,
  onChangeRuntime: vi.fn(),
  onChangeCommand: vi.fn(),
  onChangeTitle: vi.fn(),
  onChangeActorNotes: vi.fn(),
  onChangeCapabilityAutoloadText: vi.fn(),
  onSaveAsProfile: vi.fn(),
  onSave: vi.fn(),
  onSaveAndRestart: vi.fn(),
  onCancel: vi.fn(),
});
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.useFakeTimers();
  useUIStore.setState({ canAccessGlobalSettings: false });
  useModalStore.getState().closeModal("settings");
  mocks.load.mockResolvedValue({
    ok: true,
    result: {
      group_id: "g_fixture",
      actor_id: "dot",
      enabled: true,
      connected: false,
      subscribed: false,
      phase: "unconfigured",
      pending_count: 0,
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  useUIStore.setState({ canAccessGlobalSettings: null });
});
it.each(["chatgpt_dots", "web_model"] as const)(
  "keeps a Group writer's %s form usable without instance-connection reads or actions",
  async (runtime) => {
    await act(async () =>
      root.render(<ActorConfigModal {...props()} runtime={runtime} savedRuntime={runtime} />),
    );
    await act(async () => vi.advanceTimersByTimeAsync(4_000));
    expect(mocks.load).not.toHaveBeenCalled();
    expect(mocks.env).not.toHaveBeenCalled();
    const prefix = runtime === "chatgpt_dots" ? "dotsActor" : "webModelActor";
    expect(host.textContent).toContain(`${prefix}.adminManaged`);
    const title = host.querySelector<HTMLInputElement>('input[placeholder="dot"]');
    expect(title?.value).toBe("Dot");
    expect(title?.disabled).toBe(false);
    const panel = host.querySelector(`section[aria-label="${prefix}.title"]`)!;
    expect(panel.querySelectorAll("button")).toHaveLength(0);
  },
);
