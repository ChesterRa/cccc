// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import * as api from "../../services/api";
import { useGroupStore, useModalStore, useUIStore } from "../../stores";
import i18n, { i18nReady } from "../../i18n";
import { useAppModalActorActions } from "./useAppModalActorActions";

it("creates a DeepSeek agent and its Profile with the connection only in private values", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // Saving a new agent as a Profile asks for its name.
  vi.stubGlobal(
    "prompt",
    vi.fn(() => "DeepSeek fixture"),
  );
  await i18nReady;
  const originalGroup = useGroupStore.getState();
  const originalUI = useUIStore.getState();
  const originalModal = useModalStore.getState();
  useGroupStore.setState({ selectedGroupId: "g", actors: [], groups: [{ group_id: "g" }] });
  useUIStore.setState({ activeTab: "chat", busy: "", notice: null });
  const addActor = vi
    .spyOn(api, "addActor")
    .mockResolvedValue({
      ok: true,
      result: { actor: { id: "deepseek-1", runtime: "deepseek" } },
    } as never);
  const upsertProfile = vi
    .spyOn(api, "upsertActorProfile")
    .mockResolvedValue({
      ok: true,
      result: { profile: { id: "profile-1", name: "DeepSeek fixture", runtime: "deepseek" } },
    } as never);
  vi.spyOn(api, "listActorProfiles").mockResolvedValue({
    ok: true,
    result: { profiles: [] },
  } as never);
  const profileEnv = vi
    .spyOn(api, "updateActorProfilePrivateEnv")
    .mockResolvedValue({ ok: true, result: { keys: [] } } as never);
  const args: Parameters<typeof useAppModalActorActions>[0] = {
    selectedGroupId: "g",
    actors: [],
    runtimes: [],
    runtimeDetectionStatus: "ready",
    busy: "",
    setBusy: useUIStore.getState().setBusy,
    showError: vi.fn(),
    showNotice: vi.fn(),
    addActorModalOpen: true,
    editingActor: null,
    setEditingActor: vi.fn(),
    closeModal: vi.fn(),
    refreshActors: vi.fn().mockResolvedValue(undefined),
    editActorRuntime: "codex",
    editActorCommand: "",
    editActorTitle: "",
    editActorNotes: "",
    editActorCapabilityAutoloadText: "",
    setEditActorNotes: vi.fn(),
    newActorId: "deepseek-1",
    newActorRole: "peer",
    newActorRuntime: "deepseek",
    newActorCommand: "",
    newActorUseDefaultCommand: true,
    // A model typed in the general private-value box, then reset with Use default.
    newActorSecretsSetText: "DEEPSEEK_MODEL=typed-model\nOTHER_FIXTURE=1",
    newActorCapabilityAutoloadText: "",
    newActorNotes: "",
    newActorUseProfile: false,
    newActorProfileId: "",
    setAddActorError: vi.fn(),
    resetAddActorForm: vi.fn(),
    t: i18n.getFixedT("en", ["actors", "chat", "modals"]),
  };
  const connection = {
    setVars: { DEEPSEEK_API_KEY: "sk-fixture", DEEPSEEK_BASE_URL: "https://gateway.example/v1" },
    unsetKeys: ["DEEPSEEK_MODEL"],
    clear: false,
  };
  const expected = {
    OTHER_FIXTURE: "1",
    DEEPSEEK_API_KEY: "sk-fixture",
    DEEPSEEK_BASE_URL: "https://gateway.example/v1",
  };
  let actions: ReturnType<typeof useAppModalActorActions>;
  function Probe() {
    actions = useAppModalActorActions(args);
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Probe />));
    await act(async () => {
      await actions.handleAddActor(null, "default", "", connection);
    });
    expect(addActor).toHaveBeenCalledOnce();
    const [, , , runtime, command, envPrivate] = addActor.mock.calls[0];
    expect(runtime).toBe("deepseek");
    expect(command).toBe("");
    expect(envPrivate).toEqual(expected);

    await act(async () => {
      await actions.handleSaveNewActorAsProfile(connection, "default");
    });
    expect(upsertProfile).toHaveBeenCalledOnce();
    expect(JSON.stringify(upsertProfile.mock.calls[0])).not.toContain("sk-fixture");
    expect(profileEnv).toHaveBeenCalledWith("profile-1", expected, [], false);
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState(originalGroup, true);
    useUIStore.setState(originalUI, true);
    useModalStore.setState(originalModal, true);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  }
});
