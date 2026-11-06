// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, LocalSessionSource } from "../../types";
import type { EditActorConfigProps } from "./ActorConfigModal";
import { useGroupStore, useUIStore } from "../../stores";

const mocks = vi.hoisted(() => ({
  fetchLocalSessions: vi.fn(),
  fetchActors: vi.fn(),
  fetchSecrets: vi.fn(),
  uploadAvatar: vi.fn(),
  switchRuntime: vi.fn(),
  refresh: vi.fn(),
  openWebSetup: vi.fn(),
}));
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: (key: string) => key }),
}));
vi.mock("../../services/api/localSessions", () => ({
  fetchLocalSessions: mocks.fetchLocalSessions,
}));
vi.mock("../../services/api", async (original) => ({
  ...(await original<typeof import("../../services/api")>()),
  fetchActors: mocks.fetchActors,
  fetchActorPrivateEnvKeys: mocks.fetchSecrets,
  uploadActorAvatar: mocks.uploadAvatar,
  switchActorRuntime: mocks.switchRuntime,
}));
vi.mock("../../features/runtimeSwitch/runtimeSwitchState", async (original) => ({
  ...(await original<typeof import("../../features/runtimeSwitch/runtimeSwitchState")>()),
  refreshRuntimeSwitchGroup: mocks.refresh,
  openWebSetup: mocks.openWebSetup,
}));
vi.mock("../SelectCombobox", () => ({
  SelectCombobox: ({
    value,
    onChange,
    items,
    ariaLabel,
    disabled,
  }: {
    value: string;
    onChange: (value: string) => void;
    items: Array<{ value: string; label: string; disabled?: boolean }>;
    ariaLabel: string;
    disabled?: boolean;
  }) => (
    <select
      aria-label={ariaLabel}
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value)}
    >
      {items.map((item) => (
        <option key={item.value} value={item.value} disabled={item.disabled}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));
vi.mock("./RuntimeSelector", () => ({ RuntimeSelector: () => null }));
vi.mock("../CapabilityPicker", () => ({ CapabilityPicker: () => <div>capability-picker</div> }));

import { ActorConfigModal } from "./ActorConfigModal";

const actor: Actor = {
  id: "codex-1",
  generation: "generation-1",
  updated_at: "before",
  title: "Original",
  runtime: "codex",
  external_session_id: "codex:old",
  default_scope_key: "s_repo",
  capability_autoload: ["already-saved"],
  enabled: true,
};
const source = (id: string): LocalSessionSource => ({
  source_id: id,
  native_session_id: `${id}-session`,
  title: id,
  runtime: "codex",
  cwd: "/repo/subdir",
  connected: true,
});
let host: HTMLDivElement;
let root: Root;
let originalGroup: ReturnType<typeof useGroupStore.getState>;
let originalUI: ReturnType<typeof useUIStore.getState>;
let config: EditActorConfigProps;
const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (item) => item.textContent === label && !item.closest("[hidden]"),
  )!;
const render = async () => {
  await act(async () => root.render(<ActorConfigModal {...config} />));
};

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  originalGroup = useGroupStore.getState();
  originalUI = useUIStore.getState();
  useGroupStore.setState({
    selectedGroupId: "g",
    groupDoc: { group_id: "g", runtime_switches: [] },
    runtimeDetectionStatus: "ready",
  });
  useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
  mocks.fetchLocalSessions.mockResolvedValue({
    ok: true,
    result: { sources: [source("codex:old"), source("codex:new")] },
  });
  mocks.fetchActors.mockResolvedValue({ ok: true, result: { actors: [actor] } });
  mocks.fetchSecrets.mockResolvedValue({ ok: true, result: { keys: [] } });
  mocks.refresh.mockResolvedValue(undefined);
  mocks.openWebSetup.mockReturnValue("opened");
  mocks.switchRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "stopped",
      from: { runtime: "codex" },
      to: { runtime: "codex" },
    },
  });
  config = {
    mode: "edit",
    isOpen: true,
    isDark: false,
    busy: "",
    groupId: "g",
    actorId: actor.id,
    groupRole: "foreman",
    savedActor: actor,
    savedRuntime: "codex",
    isRunning: true,
    runtime: "codex",
    command: "",
    title: "Original",
    actorNotes: "Original notes",
    savedActorNotes: "Original notes",
    capabilityAutoloadText: "already-saved",
    scopes: [{ scope_key: "s_repo", url: "/repo" }],
    activeScopeKey: "s_repo",
    runtimes: [
      { name: "codex", display_name: "Codex", available: true, recommended_command: "codex" },
    ],
    actorProfiles: [],
    actorProfilesBusy: false,
    onChangeRuntime: vi.fn(),
    onChangeCommand: vi.fn(),
    onChangeTitle: vi.fn(),
    onChangeActorNotes: vi.fn(),
    onChangeCapabilityAutoloadText: vi.fn(),
    onSave: vi.fn(),
    onSaveAndRestart: vi.fn(),
    onSaveBasics: vi.fn().mockResolvedValue(undefined),
    onSaveAsProfile: vi.fn(),
    onCancel: vi.fn(),
  };
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useGroupStore.setState(originalGroup, true);
  useUIStore.setState(originalUI, true);
  vi.clearAllMocks();
});

it("uses the shared basics and avatar editor without reading native secrets or exposing managed settings", async () => {
  await render();
  expect(host.querySelector('[aria-label="uploadAvatar"]')).not.toBeNull();
  expect(host.querySelector("[data-role-badge]")?.textContent).toBe("groupRoleForeman");
  expect(host.textContent).toContain("rolePreset");
  expect(host.textContent).toContain("/repo/subdir");
  expect(host.textContent).toContain("nativeSession.configHint");
  expect(host.textContent).not.toContain("sectionAdvanced");
  expect(host.querySelector('select[aria-label="workingDirectory"]')).toBeNull();
  expect(mocks.fetchSecrets).not.toHaveBeenCalled();
});

it("keeps a managed switch draft separate from native metadata saves", async () => {
  await render();
  expect(host.textContent?.match(/runtimeSwitch\.sourceHint/g)).toHaveLength(1);
  expect(host.textContent).not.toContain("sectionRuntimeHint");
  await act(async () => button("runtimeSwitch.managed").click());
  expect(host.textContent).toContain("sectionRuntimeHint");
  config = { ...config, title: "Changed" };
  await render();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(host.textContent).toContain("runtimeSwitch.dirty");
  expect(mocks.switchRuntime).not.toHaveBeenCalled();
  await act(async () => button("common:save").click());
  expect(config.onSaveBasics).toHaveBeenCalledOnce();
  expect(config.onSave).not.toHaveBeenCalled();
  expect(config.onSaveAndRestart).not.toHaveBeenCalled();
  expect(config.onChangeRuntime).not.toHaveBeenCalled();
  expect(config.onChangeCommand).not.toHaveBeenCalled();
});

it("switches a native member to a managed session using an explicit isolated target", async () => {
  await render();
  await act(async () => button("runtimeSwitch.managed").click());
  await act(async () => button("runtimeSwitch.switch").click());
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchRuntime).toHaveBeenCalledWith(
    "g",
    "codex-1",
    expect.objectContaining({
      expected_actor_generation: "generation-1",
      expected_actor_updated_at: "before",
      expected_source_id: "codex:old",
      target: expect.objectContaining({
        kind: "managed",
        patch: expect.objectContaining({ runtime: "codex" }),
      }),
    }),
  );
  expect(config.onSave).not.toHaveBeenCalled();
  expect(config.onSaveAndRestart).not.toHaveBeenCalled();
});

it("keeps connection setup usable outside the locked editor after a native-to-Web switch", async () => {
  mocks.switchRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "started",
      from: { runtime: "codex" },
      to: { runtime: "web_model" },
    },
  });
  await render();
  await act(async () => button("runtimeSwitch.web").click());
  await act(async () => button("runtimeSwitch.switch").click());
  await act(async () => button("runtimeSwitch.confirm").click());

  expect(host.querySelector("fieldset")?.disabled).toBe(true);
  const setup = button("runtimeSwitch.openWebSetup");
  expect(setup.closest("fieldset")).toBeNull();
  expect(setup.disabled).toBe(false);
  await act(async () => {
    setup.click();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(config.onCancel).toHaveBeenCalledOnce();
  expect(mocks.openWebSetup).toHaveBeenCalledWith("g", "codex-1", "web_model", "generation-1");
});

it("does not offer an old switch's setup in a different editor after a delayed refresh", async () => {
  let finishRefresh!: () => void;
  mocks.refresh.mockReturnValueOnce(
    new Promise<void>((resolve) => {
      finishRefresh = resolve;
    }),
  );
  mocks.switchRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "started",
      from: { runtime: "codex" },
      to: { runtime: "web_model" },
    },
  });
  await render();
  await act(async () => button("runtimeSwitch.web").click());
  await act(async () => button("runtimeSwitch.switch").click());
  await act(async () => button("runtimeSwitch.confirm").click());
  config = { ...config, isOpen: false };
  await render();
  config = { ...config, isOpen: true, actorId: "other", savedActor: { ...actor, id: "other" } };
  await render();
  await act(async () => finishRefresh());
  expect(button("runtimeSwitch.openWebSetup")).toBeUndefined();
});

it("waits for notes and blocks avatar or switch actions while completion is pending", async () => {
  config = { ...config, actorNotesBusy: true, savedActorNotes: undefined };
  await render();
  expect(button("common:done").disabled).toBe(true);
  expect(host.querySelector("textarea")?.disabled).toBe(true);
  await act(async () =>
    useGroupStore.setState({
      groupDoc: {
        group_id: "g",
        runtime_switches: [
          { actor_id: "codex-1", actor_generation: "generation-1", pending: true },
        ],
      },
    }),
  );
  expect(host.querySelector<HTMLButtonElement>('[aria-label="uploadAvatar"]')?.disabled).toBe(true);
  expect(button("runtimeSwitch.switch").disabled).toBe(true);
  expect(host.textContent).toContain("runtimeSwitch.pending");
});

it("preserves the native metadata draft on save failure and cannot close during a save", async () => {
  config = {
    ...config,
    title: "Changed",
    onSaveBasics: vi.fn().mockRejectedValue(new Error("Synthetic save failure")),
  };
  await render();
  await act(async () => button("common:save").click());
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("Synthetic save failure");
  expect(host.querySelector<HTMLInputElement>('input[value="Changed"]')?.value).toBe("Changed");
  expect(host.querySelector("textarea")?.value).toBe("Original notes");
  expect(config.onSaveBasics).toHaveBeenCalledOnce();

  config = { ...config, busy: "actor-update" };
  await render();
  expect(host.querySelector("fieldset")?.disabled).toBe(true);
  expect(host.querySelector<HTMLButtonElement>('[aria-label="common:close"]')?.disabled).toBe(true);
  await act(async () =>
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })),
  );
  expect(config.onCancel).not.toHaveBeenCalled();
});

it("switches between native sessions without submitting the metadata form", async () => {
  await render();
  const sessions = host.querySelector<HTMLSelectElement>(
    'select[aria-label="runtimeSwitch.source"]',
  )!;
  await act(async () => {
    sessions.value = "codex:new";
    sessions.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => button("runtimeSwitch.switch").click());
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchRuntime).toHaveBeenCalledWith(
    "g",
    "codex-1",
    expect.objectContaining({
      expected_source_id: "codex:old",
      target: {
        kind: "native",
        source_id: "codex:new",
        expected_native_session_id: "codex:new-session",
      },
    }),
  );
  expect(config.onSaveBasics).not.toHaveBeenCalled();
});

it("does not fetch the admin-only session list for a non-admin editor", async () => {
  useUIStore.setState({ isWebAdmin: false });
  await render();
  expect(mocks.fetchLocalSessions).not.toHaveBeenCalled();
  expect(mocks.fetchSecrets).not.toHaveBeenCalled();
  expect(button("runtimeSwitch.managed").disabled).toBe(true);
  expect(host.querySelector('[aria-label="uploadAvatar"]')).not.toBeNull();
});

it("does not apply a delayed avatar response to a different editor visit", async () => {
  let finish!: (value: { ok: true; result: { actor: Actor } }) => void;
  mocks.uploadAvatar.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  config = { ...config, onAvatarChanged: vi.fn().mockResolvedValue(true) };
  await render();
  const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
  Object.defineProperty(input, "files", {
    configurable: true,
    value: [new File(["png"], "avatar.png", { type: "image/png" })],
  });
  await act(async () => input.dispatchEvent(new Event("change", { bubbles: true })));
  expect(host.querySelector<HTMLButtonElement>('[aria-label="common:close"]')?.disabled).toBe(true);

  config = { ...config, actorId: "other", savedActor: { ...actor, id: "other" }, title: "Other" };
  await render();
  await act(async () => finish({ ok: true, result: { actor } }));
  expect(config.onAvatarChanged).not.toHaveBeenCalled();
  expect(host.textContent).not.toContain("runtimeSwitch.conflict");
  expect(host.querySelector<HTMLButtonElement>('[aria-label="common:close"]')?.disabled).toBe(
    false,
  );
});

it("retains a native member's managed target draft across category changes", async () => {
  await render();
  await act(async () => button("runtimeSwitch.managed").click());
  const command = host.querySelector<HTMLInputElement>('input[placeholder="codex"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      command,
      "codex --fixture",
    );
    command.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("runtimeSwitch.native").click());
  await act(async () => button("runtimeSwitch.managed").click());
  expect(host.querySelector<HTMLInputElement>('input[placeholder="codex"]')?.value).toBe(
    "codex --fixture",
  );
  expect(config.onChangeRuntime).not.toHaveBeenCalled();
  expect(config.onChangeCommand).not.toHaveBeenCalled();
});
