// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import * as api from "../../services/api";
import * as switchState from "../../features/runtimeSwitch/runtimeSwitchState";
import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import i18n, { i18nReady } from "../../i18n";
import type { Actor } from "../../types";
import { useAppModalActorActions } from "./useAppModalActorActions";

it("switches a managed provider without first mutating or restarting the old configuration", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  await i18nReady;
  const originalGroup = useGroupStore.getState();
  const originalUI = useUIStore.getState();
  const originalModal = useModalStore.getState();
  const actor: Actor = {
    id: "worker",
    generation: "generation-1",
    updated_at: "2026-01-01T00:00:00Z",
    title: "Worker",
    runtime: "codex",
    command: ["codex"],
    enabled: true,
  };
  useGroupStore.setState({ selectedGroupId: "g", actors: [actor], groups: [{ group_id: "g" }] });
  useUIStore.setState({ activeTab: "chat", busy: "", notice: null });
  useModalStore.getState().setEditingActor(actor);
  vi.spyOn(api, "fetchGroupPrompts").mockResolvedValue({
    ok: true,
    result: {
      help: { kind: "help", source: "builtin", filename: "help.md", content: "" },
      preamble: { kind: "preamble", source: "builtin", filename: "preamble.md", content: "" },
    },
  });
  const update = vi.spyOn(api, "updateActor");
  const restart = vi.spyOn(api, "restartActor");
  vi.spyOn(switchState, "refreshRuntimeSwitchGroup").mockResolvedValue(undefined);
  const switchRuntime = vi
    .spyOn(api, "switchActorRuntime")
    .mockResolvedValue({
      ok: true,
      result: {
        group_id: "g",
        actor_id: "worker",
        op_id: "op-1",
        switched: true,
        from: { kind: "managed", runtime: "codex" },
        to: { kind: "managed", runtime: "claude" },
        completion: "done",
        start: "started",
      },
    });
  const args: Parameters<typeof useAppModalActorActions>[0] = {
    selectedGroupId: "g",
    actors: [actor],
    runtimes: [],
    runtimeDetectionStatus: "ready",
    busy: "",
    setBusy: useUIStore.getState().setBusy,
    showError: vi.fn(),
    showNotice: vi.fn(),
    addActorModalOpen: false,
    editingActor: actor,
    setEditingActor: vi.fn(),
    closeModal: vi.fn(),
    refreshActors: vi.fn().mockResolvedValue(undefined),
    editActorRuntime: "claude",
    editActorCommand: "claude",
    editActorTitle: "Worker",
    editActorNotes: "",
    editActorCapabilityAutoloadText: "",
    setEditActorNotes: vi.fn(),
    newActorId: "",
    newActorRole: "peer",
    newActorRuntime: "codex",
    newActorCommand: "codex",
    newActorUseDefaultCommand: true,
    newActorSecretsSetText: "",
    newActorCapabilityAutoloadText: "",
    newActorNotes: "",
    newActorUseProfile: false,
    newActorProfileId: "",
    setAddActorError: vi.fn(),
    resetAddActorForm: vi.fn(),
    t: i18n.getFixedT("en", ["actors", "chat", "modals"]),
  };
  let actions: ReturnType<typeof useAppModalActorActions>;
  function Probe() {
    actions = useAppModalActorActions(args);
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Probe />));
    await act(async () =>
      actions.handleSaveEditActorAndRestart({
        mode: "custom",
        setVars: {},
        unsetKeys: [],
        clear: false,
        capabilityAutoload: [],
        runtimeSwitch: { start: true },
      }),
    );
    expect(switchRuntime).toHaveBeenCalledOnce();
    expect(switchRuntime).toHaveBeenCalledWith(
      "g",
      "worker",
      expect.objectContaining({
        expected_actor_generation: "generation-1",
        expected_actor_updated_at: "2026-01-01T00:00:00Z",
        target: expect.objectContaining({
          kind: "managed",
          patch: expect.objectContaining({ runtime: "claude" }),
        }),
      }),
    );
    expect(update).not.toHaveBeenCalled();
    expect(restart).not.toHaveBeenCalled();
  } finally {
    await act(async () => root.unmount());
    useUIStore.getState().dismissNotice();
    useGroupStore.setState(originalGroup, true);
    useUIStore.setState(originalUI, true);
    useModalStore.setState(originalModal, true);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
