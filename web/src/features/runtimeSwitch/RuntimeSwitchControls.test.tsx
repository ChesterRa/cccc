// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, ActorProfile, LocalSessionSource } from "../../types";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";

const mocks = vi.hoisted(() => ({
  fetchLocalSessions: vi.fn(),
  fetchActors: vi.fn(),
  switchActorRuntime: vi.fn(),
  refresh: vi.fn(),
}));
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({
    t: (key: string, options?: { name?: string; from?: string; to?: string }) =>
      options?.from && options.to
        ? `${options.from} → ${options.to}`
        : options?.name
          ? `${key}:${options.name}`
          : key,
  }),
}));
vi.mock("../../services/api/localSessions", () => ({
  fetchLocalSessions: mocks.fetchLocalSessions,
}));
vi.mock("../../services/api", () => ({
  fetchActors: mocks.fetchActors,
  switchActorRuntime: mocks.switchActorRuntime,
}));
vi.mock("./runtimeSwitchState", () => ({ refreshRuntimeSwitchGroup: mocks.refresh }));
vi.mock("../../components/SelectCombobox", () => ({
  SelectCombobox: ({
    value,
    onChange,
    items,
    ariaLabel,
  }: {
    value: string;
    onChange: (value: string) => void;
    ariaLabel: string;
    items: Array<{ value: string; label: string; description?: string; disabled?: boolean }>;
  }) => (
    <select aria-label={ariaLabel} value={value} onChange={(event) => onChange(event.target.value)}>
      <option value="" />
      {items.map((item) => (
        <option
          key={item.value}
          value={item.value}
          title={item.description}
          disabled={item.disabled}
        >
          {item.label}
        </option>
      ))}
    </select>
  ),
}));
vi.mock("../../components/modals/RuntimeSelector", () => ({ RuntimeSelector: () => null }));
vi.mock("../../components/modals/AcpRuntimeMode", () => ({ AcpRuntimeMode: () => null }));
vi.mock("../../hooks/useModalA11y", () => ({
  useModalA11y: () => ({ modalRef: { current: null } }),
}));

import { RuntimeSwitchControls } from "./RuntimeSwitchControls";
import { useGroupStore } from "../../stores";

const actor: Actor = {
  id: "old",
  generation: "generation-1",
  updated_at: "2026-01-01T00:00:00Z",
  external_session_id: "codex:old",
  runtime: "codex",
  title: "Old member",
};
const source = (source_id: string, title: string): LocalSessionSource => ({
  source_id,
  title,
  runtime: "codex",
  native_session_id: `${source_id}-session`,
  cwd: "/fixture",
  connected: true,
});
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  mocks.fetchLocalSessions.mockResolvedValue({
    ok: true,
    result: {
      sources: [
        source("codex:old", "old"),
        source("codex:new", "new"),
        source("codex:used", "used"),
      ],
    },
  });
  mocks.fetchActors.mockResolvedValue({
    ok: true,
    result: {
      actors: [actor, { id: "other", title: "Second", external_session_id: "codex:used" }],
    },
  });
  mocks.refresh.mockResolvedValue(undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.clearAllMocks();
  useGroupStore.setState({ runtimeDetectionStatus: "idle" });
});
const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (item) => item.textContent === label,
  )!;
async function render(
  dirty = false,
  onSaveCurrentProfile?: () => Promise<boolean>,
  onCurrentSourceDirectory?: (directory: string) => void,
) {
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="native"
        isDark={false}
        dirty={dirty}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
        onSaveCurrentProfile={onSaveCurrentProfile}
        onCurrentSourceDirectory={onCurrentSourceDirectory}
      />,
    );
  });
}
async function chooseNew() {
  const select = host.querySelector<HTMLSelectElement>(
    'select[aria-label="runtimeSwitch.source"]',
  )!;
  await act(async () => {
    select.value = "codex:new";
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

it("disables an occupied source and switches the selected session with confirmed identities", async () => {
  const onCurrentSourceDirectory = vi.fn();
  mocks.switchActorRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "native",
      from: { runtime: "codex" },
      to: { runtime: "codex" },
    },
  });
  await render(false, undefined, onCurrentSourceDirectory);
  expect(onCurrentSourceDirectory).toHaveBeenCalledWith("/fixture");
  expect(host.querySelector<HTMLOptionElement>('option[value="codex:used"]')?.disabled).toBe(true);
  expect(host.querySelector<HTMLOptionElement>('option[value="codex:used"]')?.title).toContain(
    "runtimeSwitch.sourceInUse:Second (other)",
  );
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("workingDirectory: /fixture");
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchActorRuntime).toHaveBeenCalledWith("g", "old", {
    expected_actor_generation: "generation-1",
    expected_actor_updated_at: "2026-01-01T00:00:00Z",
    expected_source_id: "codex:old",
    target: {
      kind: "native",
      source_id: "codex:new",
      expected_native_session_id: "codex:new-session",
    },
  });
  expect(mocks.refresh).toHaveBeenCalledWith("g");
});

it("does not open the switch confirmation when existing edits are unsaved", async () => {
  await render(true);
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(host.textContent).toContain("runtimeSwitch.dirty");
  expect(mocks.switchActorRuntime).not.toHaveBeenCalled();
});

it("blocks another attempt when the outcome is unknown", async () => {
  mocks.switchActorRuntime.mockResolvedValue({
    ok: false,
    error: { code: "actor_switch_unknown", message: "lost" },
  });
  const unknown = vi.fn();
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="native"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
        onOutcomeUnknown={unknown}
      />,
    );
  });
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(unknown).toHaveBeenCalledOnce();
  expect(button("runtimeSwitch.switch").disabled).toBe(true);
  expect(host.textContent).toContain("runtimeSwitch.unknown");
});

it("does not switch after a failed attempt to save the current managed Profile", async () => {
  const saveProfile = vi.fn().mockResolvedValue(false);
  const managed = { ...actor, external_session_id: "", runtime: "codex" };
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={managed}
        targetKind="native"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
        onSaveCurrentProfile={saveProfile}
      />,
    );
  });
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("runtimeSwitch.profileHint");
  await act(async () => button("runtimeSwitch.saveProfile").click());
  expect(button("runtimeSwitch.confirm").disabled).toBe(true);
  expect(document.body.textContent).toContain("runtimeSwitch.profileSaveFailed");
  expect(mocks.switchActorRuntime).not.toHaveBeenCalled();
});

it("allows a fixed-directory managed member to switch to a native session", async () => {
  const managed = { ...actor, external_session_id: "", default_scope_key: "scope-managed" };
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={managed}
        targetKind="native"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
      />,
    );
  });
  await chooseNew();
  expect(button("runtimeSwitch.switch").disabled).toBe(false);
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain(
    "runtimeSwitch.managed · codex → runtimeSwitch.native · codex · new",
  );
  expect(document.body.textContent).toContain("workingDirectory: /fixture");
  expect(button("runtimeSwitch.confirm").disabled).toBe(false);
});

it("keeps the linked Profile and does not offer to duplicate it on a native switch", async () => {
  const saveProfile = vi.fn().mockResolvedValue(true);
  const managed = { ...actor, external_session_id: "", profile_id: "existing", runtime: "codex" };
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={managed}
        targetKind="native"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
        onSaveCurrentProfile={saveProfile}
      />,
    );
  });
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(
    [...document.querySelectorAll("button")].some(
      (item) => item.textContent === "runtimeSwitch.saveProfile",
    ),
  ).toBe(false);
  expect(document.body.textContent).toContain("runtimeSwitch.unlinkProfile:existing");
  expect(saveProfile).not.toHaveBeenCalled();
});

it("warns that Web Model secrets will be deleted without suggesting an unavailable backup", async () => {
  const webActor = { ...actor, external_session_id: "", runtime: "web_model" };
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={webActor}
        targetKind="native"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
        onSaveCurrentProfile={vi.fn().mockResolvedValue(true)}
      />,
    );
  });
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("runtimeSwitch.clearWebConfig");
  expect(document.body.textContent).not.toContain("runtimeSwitch.profileHint");
  expect(
    [...document.querySelectorAll("button")].some(
      (item) => item.textContent === "runtimeSwitch.saveProfile",
    ),
  ).toBe(false);
});

it("can switch to a stopped managed configuration without sending private values", async () => {
  useGroupStore.setState({ runtimeDetectionStatus: "ready" });
  mocks.switchActorRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "stopped",
      from: { runtime: "codex" },
      to: { runtime: "codex" },
    },
  });
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="managed"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[
          { name: "codex", display_name: "Codex", available: true, recommended_command: "codex" },
        ]}
        profiles={[]}
        profilesBusy={false}
      />,
    );
  });
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain(
    "runtimeSwitch.native · codex → runtimeSwitch.managed · codex",
  );
  const checkbox = document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  await act(async () => checkbox.click());
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchActorRuntime).toHaveBeenCalledWith(
    "g",
    "old",
    expect.objectContaining({
      target: {
        kind: "managed",
        patch: { runtime: "codex", runtime_mode: "default", command: "codex" },
        start: false,
      },
    }),
  );
});

it("offers Web Model connection setup only after the switched member is refreshed", async () => {
  const setupReady = vi.fn();
  let finishRefresh!: () => void;
  mocks.refresh.mockReturnValueOnce(
    new Promise<void>((resolve) => {
      finishRefresh = resolve;
    }),
  );
  mocks.switchActorRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "started",
      from: { runtime: "codex" },
      to: { runtime: "web_model" },
    },
  });
  await act(async () =>
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="web"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
        onWebSetupReady={setupReady}
      />,
    ),
  );
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("runtimeSwitch.oldNative");
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchActorRuntime).toHaveBeenCalledWith(
    "g",
    "old",
    expect.objectContaining({
      target: { kind: "managed", patch: { runtime: "web_model" }, start: true },
    }),
  );
  expect(host.textContent).toContain("runtimeSwitch.webConnectionRequired");
  expect(setupReady).not.toHaveBeenCalled();
  await act(async () => finishRefresh());
  expect(setupReady).toHaveBeenCalledWith("web_model");
});

it("warns about a previous Web conversation before switching to native", async () => {
  await act(async () =>
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={{ ...actor, external_session_id: "", runtime: "web_model" }}
        targetKind="native"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
      />,
    ),
  );
  await chooseNew();
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("runtimeSwitch.oldWeb");
  expect(document.body.textContent).not.toContain("runtimeSwitch.oldManaged");
});

it("keeps a paused native member stopped when switching to managed", async () => {
  useGroupStore.setState({ runtimeDetectionStatus: "ready" });
  mocks.switchActorRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "stopped",
      from: { runtime: "codex" },
      to: { runtime: "codex" },
    },
  });
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={{ ...actor, enabled: false }}
        targetKind="managed"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[
          { name: "codex", display_name: "Codex", available: true, recommended_command: "codex" },
        ]}
        profiles={[]}
        profilesBusy={false}
      />,
    );
  });
  await act(async () => button("runtimeSwitch.switch").click());
  const checkbox = document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  expect(checkbox.checked).toBe(true);
  expect(checkbox.disabled).toBe(true);
  expect(document.body.textContent).toContain("runtimeSwitch.pausedNativeToManaged");
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchActorRuntime).toHaveBeenCalledWith(
    "g",
    "old",
    expect.objectContaining({ target: expect.objectContaining({ start: false }) }),
  );
});

it("accepts an explicit command when runtime detection failed", async () => {
  useGroupStore.setState({ runtimeDetectionStatus: "error" });
  await act(async () => {
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="managed"
        isDark={false}
        dirty={false}
        disabled={false}
        runtimes={[]}
        profiles={[]}
        profilesBusy={false}
      />,
    );
  });
  expect(button("runtimeSwitch.switch").disabled).toBe(true);
  const command = host.querySelector<HTMLInputElement>("input")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      command,
      "/opt/tools/codex",
    );
    command.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(button("runtimeSwitch.switch").disabled).toBe(false);
});

it("confirms the custom runtime after switching back from a Profile draft", async () => {
  useGroupStore.setState({ runtimeDetectionStatus: "ready" });
  const profile: ActorProfile = {
    id: "other",
    name: "Different Profile",
    runtime: "claude",
    scope: "global",
    runner: "pty",
    command: [],
    submit: "enter",
    env: {},
    created_at: "",
    updated_at: "",
    revision: 1,
  };
  mocks.switchActorRuntime.mockResolvedValue({
    ok: true,
    result: {
      completion: "done",
      start: "stopped",
      from: { runtime: "codex" },
      to: { runtime: "codex" },
    },
  });
  await act(async () =>
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="managed"
        isDark={false}
        dirty={false}
        disabled={false}
        profiles={[profile]}
        profilesBusy={false}
        runtimes={[
          { name: "codex", display_name: "Codex", available: true, recommended_command: "codex" },
        ]}
      />,
    ),
  );
  await act(async () => button("fromActorProfile").click());
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="actorProfile"]')!;
  await act(async () => {
    select.value = actorProfileIdentityKey(profile);
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => button("customAgent").click());
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("runtimeSwitch.managed · codex");
  expect(document.body.textContent).not.toContain("runtimeSwitch.managed · Different Profile");
  await act(async () => button("runtimeSwitch.confirm").click());
  expect(mocks.switchActorRuntime).toHaveBeenCalledWith(
    "g",
    "old",
    expect.objectContaining({
      target: expect.objectContaining({
        kind: "managed",
        patch: expect.objectContaining({ runtime: "codex" }),
      }),
    }),
  );
});

it("identifies an unnamed Profile by its ID in the switch confirmation", async () => {
  const profile: ActorProfile = {
    id: "claude-profile",
    name: "",
    runtime: "claude",
    scope: "global",
    runner: "pty",
    command: [],
    submit: "enter",
    env: {},
    created_at: "",
    updated_at: "",
    revision: 1,
  };
  await act(async () =>
    root.render(
      <RuntimeSwitchControls
        groupId="g"
        actor={actor}
        targetKind="managed"
        isDark={false}
        dirty={false}
        disabled={false}
        profiles={[profile]}
        profilesBusy={false}
        runtimes={[]}
      />,
    ),
  );
  await act(async () => button("fromActorProfile").click());
  const select = host.querySelector<HTMLSelectElement>('select[aria-label="actorProfile"]')!;
  await act(async () => {
    select.value = actorProfileIdentityKey(profile);
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
  await act(async () => button("runtimeSwitch.switch").click());
  expect(document.body.textContent).toContain("runtimeSwitch.managed · claude-profile");
});
