// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../types";

const mocks = vi.hoisted(() => ({ add: vi.fn(), refresh: vi.fn(), t: (key: string) => key }));
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: mocks.t }),
}));
vi.mock("../services/api", () => ({
  addActor: (...args: unknown[]) => mocks.add(...args),
  listActorProfiles: async () => ({ ok: true, result: { profiles: [] } }),
}));
vi.mock("./modals/ActorConfigModal", () => ({
  ActorConfigModal: ({
    mode,
    isOpen,
    onCreate,
  }: {
    mode: string;
    isOpen: boolean;
    onCreate?: () => Promise<boolean>;
  }) =>
    mode === "create" && isOpen ? (
      <button onClick={() => void onCreate?.()}>Create Dot</button>
    ) : null,
}));
vi.mock("./SearchModal", () => ({ SearchModal: () => null }));
vi.mock("./layout/MobileMenuSheet", () => ({ MobileMenuSheet: () => null }));
vi.mock("./modals/CreateGroupModal", () => ({ CreateGroupModal: () => null }));
vi.mock("./modals/GroupEditModal", () => ({ GroupEditModal: () => null }));
vi.mock("./modals/InboxModal", () => ({ InboxModal: () => null }));
vi.mock("./presentation/PresentationPinModal", () => ({ PresentationPinModal: () => null }));
vi.mock("./modals/RelayMessageModal", () => ({ RelayMessageModal: () => null }));
vi.mock("./modals/RecipientsModal", () => ({ RecipientsModal: () => null }));
import { AppModals } from "./AppModals";
import { useFormStore, useGroupStore, useModalStore, useUIStore } from "../stores";
import { getChatSession } from "../stores/useUIStore";
import { finishWorkspaceNavigation, useWorkspaceNavigation } from "../stores/workspaceNavigation";
import { shouldMountAppModals } from "../utils/appLazyMount";
import { useAppTabState } from "../hooks/useAppTabState";

const created = {
  id: "dot-created",
  runtime: "chatgpt_dots",
  enabled: true,
  running: false,
  runner_effective: "headless",
  runtime_state_source: "terminal",
  role: "peer",
} as Actor;
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  useModalStore.setState({
    modals: { ...useModalStore.getState().modals, addActor: true },
    editingActor: null,
  });
  useUIStore.setState({ activeTab: "chat", chatSessions: {}, busy: "", isSmallScreen: true });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  useFormStore.getState().resetAddActorForm();
  useFormStore.setState({ newActorRuntime: "chatgpt_dots", newActorId: "dot-created" });
  useGroupStore.setState({
    selectedGroupId: "g_a",
    actors: [],
    groups: [],
    events: [],
    groupDoc: null,
    refreshActors: mocks.refresh,
    runtimes: [],
  });
  mocks.add.mockResolvedValue({ ok: true, result: { actor: created } });
  mocks.refresh.mockImplementation(async (groupId: string) => {
    if (useGroupStore.getState().selectedGroupId === groupId)
      useGroupStore.setState({ actors: [created] });
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});
function AppLifecycleHost() {
  const modals = useModalStore((state) => state.modals);
  const groupId = useGroupStore((state) => state.selectedGroupId);
  const actors = useGroupStore((state) => state.actors);
  const activeTab = useUIStore((state) => state.activeTab);
  const { onDotsActorCreated } = useAppTabState({
    activeTab,
    runtimeActors: actors,
    selectedGroupId: groupId,
    isSmallScreen: true,
    setActiveTab: useUIStore.getState().setActiveTab,
    setShowScrollButton: vi.fn(),
    setChatUnreadCount: vi.fn(),
  });
  return shouldMountAppModals({ modals }) ? (
    <AppModals
      codexVoice={{} as React.ComponentProps<typeof AppModals>["codexVoice"]}
      isDark={false}
      theme="light"
      textScale={100}
      ccccHome="/fixture-only"
      composerRef={createRef()}
      onStartReply={vi.fn()}
      onDotsActorCreated={onDotsActorCreated}
      onThemeChange={vi.fn()}
      onTextScaleChange={vi.fn()}
      onDeleteGroup={vi.fn()}
      fetchContext={vi.fn()}
      canManageGroups
    />
  ) : null;
}
async function render() {
  await act(async () => root.render(<AppLifecycleHost />));
}
const submit = async () => {
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
};
it("opens the actual newly created enabled Dot's pane after Actors hydrate, without connecting or starting it", async () => {
  await render();
  await submit();
  expect(mocks.add).toHaveBeenCalledWith(
    "g_a",
    "dot-created",
    "peer",
    "chatgpt_dots",
    "",
    undefined,
    { capabilityAutoload: [] },
  );
  expect(useUIStore.getState().activeTab).toBe("dot-created");
  expect(getChatSession("g_a", useUIStore.getState().chatSessions).mobileSurface).toBe("messages");
  expect(useModalStore.getState().editingActor).toBeNull();
  expect(mocks.refresh).toHaveBeenCalledWith("g_a");
});
it("waits for the existing Actor refresh to publish the created identity rather than navigating to an unknown Actor", async () => {
  mocks.refresh.mockResolvedValue(undefined);
  await render();
  await submit();
  expect(useUIStore.getState().activeTab).toBe("chat");
  await act(async () => useGroupStore.setState({ actors: [created] }));
  expect(useUIStore.getState().activeTab).toBe("dot-created");
});
it("does not navigate a different Group when an accepted creation response arrives late", async () => {
  let finish!: (value: unknown) => void;
  mocks.add.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render();
  await submit();
  await act(async () => useGroupStore.setState({ selectedGroupId: "g_b", actors: [] }));
  await act(async () => finish({ ok: true, result: { actor: created } }));
  expect(useUIStore.getState().activeTab).toBe("chat");
  expect(mocks.refresh).toHaveBeenCalledWith("g_a");
});
it("retires the creation hint if the user chooses another pane before Actor hydration", async () => {
  mocks.refresh.mockResolvedValue(undefined);
  await render();
  await submit();
  await act(async () => useUIStore.getState().setActiveTab("another-actor"));
  await act(async () => useGroupStore.setState({ actors: [created] }));
  expect(useUIStore.getState().activeTab).toBe("another-actor");
});
it("retains the existing unsaved-work navigation guard and does not change Group or active pane before confirmation", async () => {
  useWorkspaceNavigation.setState({ owners: new Set([Symbol("unsaved-work")]), pending: null });
  await render();
  await submit();
  expect(useUIStore.getState().activeTab).toBe("chat");
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  await act(async () => finishWorkspaceNavigation(true));
  expect(useUIStore.getState().activeTab).toBe("dot-created");
  expect(useGroupStore.getState().selectedGroupId).toBe("g_a");
});
