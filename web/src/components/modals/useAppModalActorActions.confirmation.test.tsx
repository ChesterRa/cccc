// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import * as api from "../../services/api";
import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import i18n, { i18nReady } from "../../i18n";
import type { Actor } from "../../types";
import { useAppModalActorActions } from "./useAppModalActorActions";

it.each([
  "update",
  "restart",
  "save_only",
  "rollback_failure",
  "ordinary_success",
  "ordinary_failure",
] as const)(
  "handles %s without losing confirmation or the original save result",
  async (waitingAt) => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.stubGlobal("confirm", () => true);
    await i18nReady;
    const originalGroup = useGroupStore.getState();
    const originalUI = useUIStore.getState();
    const originalModal = useModalStore.getState();
    const waiting = !waitingAt.startsWith("ordinary");
    const failed = waitingAt.startsWith("rollback") || waitingAt === "ordinary_failure";
    const actor: Actor = {
      id: "claude",
      title: "Claude",
      runtime: "claude",
      command: ["claude"],
      running: true,
      runtime_confirmation_required: waitingAt === "update" || waitingAt === "save_only",
    };
    useGroupStore.setState({ selectedGroupId: "g", actors: [actor], groups: [{ group_id: "g" }] });
    useUIStore.setState({ activeTab: "chat", busy: "", notice: null });
    useModalStore.getState().setEditingActor(actor);
    vi.spyOn(api, "fetchActors").mockResolvedValue({
      ok: true,
      result: {
        actors: [{ ...actor, title: "Updated Claude", runtime_confirmation_required: waiting }],
      },
    });
    vi.spyOn(api, "fetchGroupPrompts").mockResolvedValue({
      ok: true,
      result: {
        help: { kind: "help", source: "builtin", filename: "help.md", content: "" },
        preamble: { kind: "preamble", source: "builtin", filename: "preamble.md", content: "" },
      },
    });
    const update = vi
      .spyOn(api, "updateActor")
      .mockResolvedValue(
        failed
          ? {
              ok: false,
              error: { code: "rollback_failed", message: "Fixture persistence failure" },
            }
          : {
              ok: true,
              result: {
                runtime_confirmation_required: waitingAt === "update" || waitingAt === "save_only",
              },
            },
      );
    const restart = vi
      .spyOn(api, "restartActor")
      .mockResolvedValue({ ok: true, result: { runtime_confirmation_required: waiting } });
    const setEditingActor = vi.fn(useModalStore.getState().setEditingActor);
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
      setEditingActor,
      closeModal: vi.fn(),
      refreshActors: useGroupStore.getState().refreshActors,
      editActorRuntime: "claude",
      editActorCommand: "claude --new-setting",
      editActorTitle: "Updated Claude",
      editActorNotes: "",
      editActorCapabilityAutoloadText: "",
      setEditActorNotes: vi.fn(),
      newActorId: "",
      newActorRole: "peer",
      newActorRuntime: "claude",
      newActorCommand: "claude",
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
      let failure: unknown;
      await act(async () => {
        try {
          const save =
            waitingAt === "save_only"
              ? actions.handleSaveEditActorOnly
              : actions.handleSaveEditActorAndRestart;
          await save({
            mode: "custom",
            setVars: {},
            unsetKeys: [],
            clear: false,
            capabilityAutoload: [],
          });
        } catch (error) {
          failure = error;
        }
      });
      expect(update).toHaveBeenCalledOnce();
      if (failed || waitingAt === "save_only") expect(restart).not.toHaveBeenCalled();
      else expect(restart).toHaveBeenCalledExactlyOnceWith("g", "claude");
      if (failed) {
        expect(failure).toEqual(new Error("rollback_failed: Fixture persistence failure"));
        // The editor keeps its draft: nothing closes it, navigates away or offers to.
        expect(setEditingActor).not.toHaveBeenCalled();
        expect(useUIStore.getState().activeTab).toBe("chat");
        expect(useUIStore.getState().notice).toBeNull();
      } else {
        expect(failure).toBeUndefined();
        expect(setEditingActor).toHaveBeenLastCalledWith(null);
        // Waiting, the Actor's terminal opens and its own notice says what to do, so no
        // pop-up repeats it; a save for the next start still says so.
        expect(useUIStore.getState().activeTab).toBe(waiting ? actor.id : "chat");
        if (waitingAt === "save_only")
          expect(useUIStore.getState().notice?.message).toContain("Saved for the next start");
        else expect(useUIStore.getState().notice).toBeNull();
      }
      expect(args.showError).not.toHaveBeenCalled();
      expect(useUIStore.getState().busy).toBe("");
    } finally {
      await act(async () => root.unmount());
      useUIStore.getState().dismissNotice();
      useGroupStore.setState(originalGroup, true);
      useUIStore.setState(originalUI, true);
      useModalStore.setState(originalModal, true);
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  },
);
