// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../types";

const mocks = vi.hoisted(() => ({
  add: vi.fn(),
  refresh: vi.fn(),
  recipientsProps: vi.fn(),
  bind: vi.fn(),
  editSaveTarget: "",
  t: (key: string) => key,
}));
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: mocks.t }),
}));
vi.mock("../services/api", () => ({
  addActor: (...args: unknown[]) => mocks.add(...args),
  bindGrokBot: (...args: unknown[]) => mocks.bind(...args),
  fetchGroupPrompts: async () => ({ ok: true, result: { help: { content: "" } } }),
  listActorProfiles: async () => ({ ok: true, result: { profiles: [] } }),
}));
vi.mock("./modals/ActorConfigModal", () => ({
  ActorConfigModal: ({
    mode,
    isOpen,
    onCreate,
    onSave,
    scopes,
    activeScopeKey,
  }: {
    mode: string;
    isOpen: boolean;
    onCreate?: (avatar?: File | null, mode?: "default", scope?: string) => Promise<boolean>;
    scopes?: Array<{ scope_key?: string }>;
    activeScopeKey?: string;
    onSave?: (payload: {
      mode: "custom";
      setVars: {};
      unsetKeys: [];
      clear: false;
      capabilityAutoload: [];
      grokBotUrl: string;
    }) => Promise<void>;
  }) =>
    mode === "create" && isOpen ? (
      <>
        <button onClick={() => void onCreate?.()}>Create Dot</button>
        <button onClick={() => void onCreate?.(null, "default", scopes?.[1]?.scope_key)}>
          Create in second scope
        </button>
        <span data-active-scope={activeScopeKey} />
      </>
    ) : mode === "edit" && isOpen && mocks.editSaveTarget ? (
      <button
        onClick={() =>
          void onSave?.({
            mode: "custom",
            setVars: {},
            unsetKeys: [],
            clear: false,
            capabilityAutoload: [],
            grokBotUrl: mocks.editSaveTarget,
          })
        }
      >
        Save Bot URL
      </button>
    ) : null,
}));
vi.mock("./SearchModal", () => ({ SearchModal: () => null }));
vi.mock("./layout/MobileMenuSheet", () => ({ MobileMenuSheet: () => null }));
vi.mock("./modals/CreateGroupModal", () => ({ CreateGroupModal: () => null }));
vi.mock("./modals/GroupEditModal", () => ({ GroupEditModal: () => null }));
vi.mock("./modals/InboxModal", () => ({ InboxModal: () => null }));
vi.mock("./presentation/PresentationPinModal", () => ({ PresentationPinModal: () => null }));
vi.mock("./modals/RelayMessageModal", () => ({ RelayMessageModal: () => null }));
vi.mock("./modals/RecipientsModal", () => ({
  RecipientsModal: (props: unknown) => {
    mocks.recipientsProps(props);
    return null;
  },
}));
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
  mocks.editSaveTarget = "";
  mocks.bind.mockResolvedValue({ ok: true, result: {} });
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
  const recipientsEventId = useModalStore((state) => state.recipientsEventId);
  const groupId = useGroupStore((state) => state.selectedGroupId);
  const actors = useGroupStore((state) => state.actors);
  const activeTab = useUIStore((state) => state.activeTab);
  const { onNativeActorCreated } = useAppTabState({
    activeTab,
    runtimeActors: actors,
    selectedGroupId: groupId,
    isSmallScreen: true,
    setActiveTab: useUIStore.getState().setActiveTab,
    setShowScrollButton: vi.fn(),
    setChatUnreadCount: vi.fn(),
  });
  return shouldMountAppModals({ modals, recipientsEventId }) ? (
    <AppModals
      codexVoice={{} as React.ComponentProps<typeof AppModals>["codexVoice"]}
      isDark={false}
      theme="light"
      textScale={100}
      ccccHome="/fixture-only"
      composerRef={createRef()}
      onStartReply={vi.fn()}
      onNativeActorCreated={onNativeActorCreated}
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
    { capabilityAutoload: [], defaultScopeKey: "" },
  );
  expect(useUIStore.getState().activeTab).toBe("dot-created");
  expect(getChatSession("g_a", useUIStore.getState().chatSessions).mobileSurface).toBe("messages");
  expect(useModalStore.getState().editingActor).toBeNull();
  expect(mocks.refresh).toHaveBeenCalledWith("g_a");
});
it("passes the selected attached scope through the creation action", async () => {
  useGroupStore.setState({
    groupDoc: {
      group_id: "g_a",
      active_scope_key: "s_a",
      scopes: [
        { scope_key: "s_a", url: "/fixture/a" },
        { scope_key: "s_b", url: "/fixture/b" },
      ],
    },
  });
  await render();
  expect(host.querySelector('[data-active-scope="s_a"]')).not.toBeNull();
  await act(async () => host.querySelectorAll<HTMLButtonElement>("button")[1].click());
  expect(mocks.add).toHaveBeenLastCalledWith(
    "g_a",
    "dot-created",
    "peer",
    "chatgpt_dots",
    "",
    undefined,
    { capabilityAutoload: [], defaultScopeKey: "s_b" },
  );
});
it("waits for the existing Actor refresh to publish the created identity rather than navigating to an unknown Actor", async () => {
  mocks.refresh.mockResolvedValue(undefined);
  await render();
  await submit();
  expect(useUIStore.getState().activeTab).toBe("chat");
  await act(async () => useGroupStore.setState({ actors: [created] }));
  expect(useUIStore.getState().activeTab).toBe("dot-created");
});

it("takes a newly created Grok Bot to its required connection settings after hydration", async () => {
  const grok = { ...created, id: "grok-created", runtime: "grok_web_model" } as Actor;
  useFormStore.setState({ newActorRuntime: "grok_web_model", newActorId: grok.id });
  mocks.add.mockResolvedValue({ ok: true, result: { actor: grok } });
  mocks.refresh.mockResolvedValue(undefined);
  await render();
  await submit();
  expect(useModalStore.getState().editingActor).toBeNull();
  await act(async () => useGroupStore.setState({ actors: [grok] }));
  expect(useUIStore.getState().activeTab).toBe(grok.id);
  expect(useModalStore.getState().editingActor?.id).toBe(grok.id);
  expect(useModalStore.getState().editingActorSection).toBe("chatgpt");
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
it("resolves RecipientsModal metadata from the selected Group's chatByGroup bucket", async () => {
  useGroupStore
    .getState()
    .setEvents(
      [
        {
          id: "evt-reply-req",
          kind: "chat.message",
          by: "user",
          ts: "2026-10-17T00:00:00Z",
          data: { text: "Please reply", to: ["dot-created"], message_mode: "request_reply" },
          _obligation_status: {
            "dot-created": {
              replied: false,
              reply_requested: true,
              cancelled: false,
              delivery_state: "accepted",
            },
          },
        },
      ],
      "g_a",
    );
  useGroupStore.setState({ actors: [created] });
  useModalStore.setState({
    modals: { ...useModalStore.getState().modals, addActor: false },
    recipientsEventId: "evt-reply-req",
  });
  await render();
  expect(mocks.recipientsProps).toHaveBeenLastCalledWith(
    expect.objectContaining({ isOpen: true, messageMode: "request_reply", canCancelReply: true }),
  );
});

it("does not open stale Grok settings if the Actor changes while navigation awaits confirmation", async () => {
  const grok = { ...created, id: "grok-created", runtime: "grok_web_model" } as Actor;
  useFormStore.setState({ newActorRuntime: "grok_web_model", newActorId: grok.id });
  mocks.add.mockResolvedValue({ ok: true, result: { actor: grok } });
  mocks.refresh.mockImplementation(async () => useGroupStore.setState({ actors: [grok] }));
  useWorkspaceNavigation.setState({ owners: new Set([Symbol("unsaved-work")]), pending: null });
  await render();
  await submit();
  expect(useWorkspaceNavigation.getState().pending).not.toBeNull();
  await act(async () => useGroupStore.setState({ actors: [{ ...grok, runtime: "codex" }] }));
  await act(async () => finishWorkspaceNavigation(true));
  expect(useUIStore.getState().activeTab).toBe("chat");
  expect(useModalStore.getState().editingActor).toBeNull();
});
it("keeps Grok setup open after its Bot URL is saved so Routine setup can continue", async () => {
  const grok = { ...created, id: "grok-created", runtime: "grok_web_model" } as Actor;
  useFormStore.setState({ newActorRuntime: "grok_web_model", newActorId: grok.id });
  mocks.add.mockResolvedValue({ ok: true, result: { actor: grok } });
  mocks.refresh.mockImplementation(async () => useGroupStore.setState({ actors: [grok] }));
  await render();
  await submit();
  mocks.editSaveTarget = "https://grok.com/bot/00000000-0000-4000-8000-000000000001";
  await render();
  const save = [...host.querySelectorAll("button")].find(
    (button) => button.textContent === "Save Bot URL",
  )!;
  await act(async () => save.click());
  expect(mocks.bind).toHaveBeenCalledExactlyOnceWith("g_a", grok.id, mocks.editSaveTarget);
  expect(useModalStore.getState().editingActor?.id).toBe(grok.id);
});
