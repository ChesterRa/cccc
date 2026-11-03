// @vitest-environment happy-dom
import { act, createRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { RuntimeInfo, SupportedRuntime } from "../types";

// One t for every render, as i18next gives: effects that depend on it must not rerun each time.
const t = vi.hoisted(() => (key: string) => key);
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t }),
}));
vi.mock("../services/api", () => ({
  fetchGroupPrompts: async () => ({ ok: true, result: { help: { content: "" } } }),
  listActorProfiles: async () => ({ ok: true, result: { profiles: [] } }),
}));
// The form is reduced to the choices a reader can make about how the new agent runs.
vi.mock("./modals/ActorConfigModal", () => ({
  ActorConfigModal: ({
    mode,
    isOpen,
    onChangeRuntime,
    onChangeCommand,
    onChangeUseDefaultCommand,
    onChangeUseProfile,
  }: {
    mode: string;
    isOpen: boolean;
    onChangeRuntime?: (value: SupportedRuntime) => void;
    onChangeCommand?: (value: string) => void;
    onChangeUseDefaultCommand?: (value: boolean) => void;
    onChangeUseProfile?: (value: boolean) => void;
  }) =>
    mode === "create" && isOpen ? (
      <>
        <button data-pick-codex onClick={() => onChangeRuntime?.("codex")} />
        <button data-pick-pi onClick={() => onChangeRuntime?.("pi")} />
        <button
          data-own-command
          onClick={() => {
            onChangeUseDefaultCommand?.(false);
            onChangeCommand?.("/opt/codex/bin/codex");
          }}
        />
        <button data-default-command onClick={() => onChangeUseDefaultCommand?.(true)} />
        <button data-profile onClick={() => onChangeUseProfile?.(true)} />
      </>
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
import { useWorkspaceNavigation } from "../stores/workspaceNavigation";

// Only Claude Code is installed; the form opens on Codex.
const onlyClaude: RuntimeInfo[] = [
  { name: "codex", display_name: "Codex", available: false },
  { name: "claude", display_name: "Claude Code", available: true, recommended_command: "claude" },
];

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  useModalStore.setState({
    modals: { ...useModalStore.getState().modals, addActor: true },
    editingActor: null,
  });
  useUIStore.setState({ activeTab: "chat", chatSessions: {}, busy: "", isSmallScreen: true });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  useFormStore.getState().resetAddActorForm();
  useGroupStore.setState({
    selectedGroupId: "g_a",
    actors: [],
    groups: [],
    groupDoc: null,
    refreshActors: vi.fn(async () => {}),
    runtimes: [],
    runtimeDetectionStatus: "loading",
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
});

async function click(selector: string) {
  await act(async () => host.querySelector<HTMLButtonElement>(selector)!.click());
}
async function finishDetection() {
  await act(async () =>
    useGroupStore.setState({ runtimes: onlyClaude, runtimeDetectionStatus: "ready" }),
  );
}

it("shows the installed runtime once detection finishes on a form the reader has not changed", async () => {
  await finishDetection();
  expect(useFormStore.getState().newActorRuntime).toBe("claude");
});

it("keeps a runtime the reader picked while detection was running, the default one included", async () => {
  await click("[data-pick-codex]");
  await finishDetection();
  expect(useFormStore.getState().newActorRuntime).toBe("codex");
});

it("keeps another runtime the reader picked while detection was running", async () => {
  await click("[data-pick-pi]");
  await finishDetection();
  expect(useFormStore.getState().newActorRuntime).toBe("pi");
});

it("keeps the runtime that goes with the reader's own command", async () => {
  await click("[data-own-command]");
  await finishDetection();
  expect(useFormStore.getState()).toMatchObject({
    newActorRuntime: "codex",
    newActorUseDefaultCommand: false,
    newActorCommand: "/opt/codex/bin/codex",
  });
});

it("does not bring back a command typed for the old runtime after showing the installed one", async () => {
  await click("[data-own-command]");
  await click("[data-default-command]");
  await finishDetection();
  expect(useFormStore.getState()).toMatchObject({
    newActorRuntime: "claude",
    newActorUseDefaultCommand: true,
    newActorCommand: "claude",
  });
});

it("leaves the runtime alone while the reader uses a Runtime Profile", async () => {
  await click("[data-profile]");
  await finishDetection();
  expect(useFormStore.getState()).toMatchObject({
    newActorRuntime: "codex",
    newActorUseProfile: true,
  });
});
