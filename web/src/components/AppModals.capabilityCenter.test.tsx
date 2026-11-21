// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

const t = vi.hoisted(() => (key: string) => key);
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t }),
}));
vi.mock("../services/api", () => ({
  fetchGroupPrompts: async () => ({ ok: true, result: { help: { content: "" } } }),
  listActorProfiles: async () => ({ ok: true, result: { profiles: [] } }),
}));
// The center itself is covered by its own tests; here it only shows which Group it acts on.
vi.mock("./capabilities/CapabilityCenterWorkspace", () => ({
  CapabilityCenterWorkspace: ({ groupId, onClose }: { groupId: string; onClose: () => void }) => (
    <div data-center={groupId}>
      <button data-close-center onClick={onClose} />
    </div>
  ),
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
import { useGroupStore, useModalStore, useUIStore } from "../stores";
import { useWorkspaceNavigation } from "../stores/workspaceNavigation";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const center = () => host.querySelector<HTMLElement>("[data-center]");
const openCenter = (groupId: string) =>
  act(async () => useModalStore.getState().setCapabilityCenter(groupId));

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useModalStore.setState({ capabilityCenterGroupId: null });
  useUIStore.setState({ activeTab: "chat", chatSessions: {}, busy: "", isSmallScreen: false });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  useGroupStore.setState({
    selectedGroupId: "g_a",
    actors: [],
    groups: [],
    groupDoc: null,
    refreshActors: vi.fn(async () => {}),
    runtimes: [],
    runtimeDetectionStatus: "ready",
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  await act(async () =>
    root.render(
      <AppModals
        codexVoice={{} as React.ComponentProps<typeof AppModals>["codexVoice"]}
        isDark={false}
        theme="light"
        textScale={100}
        ccccHome="/fixture-only"
        composerRef={createRef()}
        onStartReply={vi.fn()}
        onNativeActorCreated={vi.fn()}
        onThemeChange={vi.fn()}
        onTextScaleChange={vi.fn()}
        onDeleteGroup={vi.fn()}
        fetchContext={vi.fn()}
        canManageGroups
      />,
    ),
  );
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useModalStore.setState({ capabilityCenterGroupId: null });
});

it("opens the capability center for the Group it was asked for and closes it from inside", async () => {
  expect(center()).toBeNull();
  await openCenter("g_a");
  expect(center()?.dataset.center).toBe("g_a");
  await act(async () => host.querySelector<HTMLButtonElement>("[data-close-center]")!.click());
  expect(center()).toBeNull();
  expect(useModalStore.getState().capabilityCenterGroupId).toBeNull();
});

it("closes the capability center when another Group is selected", async () => {
  await openCenter("g_a");
  expect(center()).not.toBeNull();
  await act(async () => useGroupStore.setState({ selectedGroupId: "g_b" }));
  expect(center()).toBeNull();
  expect(useModalStore.getState().capabilityCenterGroupId).toBeNull();
});

it("keeps a center opened without a Group while no Group is selected", async () => {
  await act(async () => useGroupStore.setState({ selectedGroupId: "" }));
  await openCenter("");
  expect(center()?.dataset.center).toBe("");
});
