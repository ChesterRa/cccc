// @vitest-environment happy-dom
import { act, createRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, ActorProfile } from "../../types";
import type { CreateActorConfigProps } from "./ActorConfigModal";

const translations = vi.hoisted(() => ({
  t: (key: string, opts?: { directory?: string }) =>
    opts?.directory ? `${key}: ${opts.directory}` : key,
}));
vi.mock("react-i18next", async (original) => ({
  ...(await original<typeof import("react-i18next")>()),
  useTranslation: () => ({ t: translations.t }),
}));
vi.mock("../CapabilityPicker", () => ({ CapabilityPicker: () => null }));
vi.mock("../RolePresetPicker", () => ({ RolePresetPicker: () => null }));
vi.mock("../ActorAvatarField", () => ({
  ActorAvatarField: ({
    onSelectFile,
    disabled,
    avatarUrl,
    resetDisabled,
    onReset,
  }: {
    onSelectFile: (file: File) => void;
    disabled?: boolean;
    avatarUrl?: string | null;
    resetDisabled?: boolean;
    onReset: () => void;
  }) => (
    <div data-avatar-url={avatarUrl || ""}>
      <button
        type="button"
        disabled={disabled}
        onClick={() => onSelectFile(new File(["avatar"], "avatar.png"))}
      >
        fixture avatar upload
      </button>
      <button type="button" disabled={disabled || resetDisabled} onClick={onReset}>
        fixture avatar reset
      </button>
    </div>
  ),
}));
vi.mock("./RuntimeSelector", () => ({ RuntimeSelector: () => null }));
vi.mock("../SelectCombobox", () => ({
  SelectCombobox: ({
    ariaLabel,
    value,
    onChange,
    items,
    disabled,
  }: {
    ariaLabel: string;
    value: string;
    onChange: (v: string) => void;
    items: Array<{ value: string; label: string }>;
    disabled?: boolean;
  }) => (
    <select
      aria-label={ariaLabel}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
    >
      {items.map((item) => (
        <option key={item.value} value={item.value}>
          {item.label}
        </option>
      ))}
    </select>
  ),
}));
vi.mock("../SearchModal", () => ({ SearchModal: () => null }));
vi.mock("../layout/MobileMenuSheet", () => ({ MobileMenuSheet: () => null }));
vi.mock("./CreateGroupModal", () => ({ CreateGroupModal: () => null }));
vi.mock("./GroupEditModal", () => ({ GroupEditModal: () => null }));
vi.mock("./InboxModal", () => ({ InboxModal: () => null }));
vi.mock("../presentation/PresentationPinModal", () => ({ PresentationPinModal: () => null }));
vi.mock("./RelayMessageModal", () => ({ RelayMessageModal: () => null }));
vi.mock("./RecipientsModal", () => ({ RecipientsModal: () => null }));
import { ActorConfigModal } from "./ActorConfigModal";
import { AppModals } from "../AppModals";
import { useFormStore, useGroupStore, useModalStore, useUIStore } from "../../stores";
import { setAuthToken } from "../../services/api/base";
import { actorProfileIdentityKey } from "../../utils/actorProfiles";
import { updateActorHelpNote } from "../../utils/helpMarkdown";
import * as switchState from "../../features/runtimeSwitch/runtimeSwitchState";

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let props: CreateActorConfigProps;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  setAuthToken("");
  useModalStore.setState(useModalStore.getInitialState());
  useFormStore.setState(useFormStore.getInitialState());
  useUIStore.setState({ busy: "", notice: null, isSmallScreen: false });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  props = {
    mode: "create",
    isOpen: true,
    isDark: false,
    busy: "",
    hasForeman: false,
    scopes: [
      { scope_key: "s_a", url: "/fixture/a", label: "Project A" },
      { scope_key: "s_b", url: "/fixture/b", label: "Project B" },
    ],
    activeScopeKey: "s_a",
    runtimes: [],
    suggestedActorId: "codex-1",
    actorId: "",
    role: "peer",
    useProfile: false,
    profileId: "",
    actorProfiles: [],
    actorProfilesBusy: false,
    runtime: "codex",
    command: "",
    useDefaultCommand: true,
    secretsSetText: "",
    capabilityAutoloadText: "",
    actorNotes: "",
    error: "",
    canSubmit: true,
    submitDisabledReason: "",
    onChangeActorId: vi.fn(),
    onChangeRole: vi.fn(),
    onChangeUseProfile: vi.fn(),
    onChangeProfileId: vi.fn(),
    onChangeRuntime: vi.fn(),
    onChangeCommand: vi.fn(),
    onChangeUseDefaultCommand: vi.fn(),
    onChangeSecretsSetText: vi.fn(),
    onChangeCapabilityAutoloadText: vi.fn(),
    onChangeActorNotes: vi.fn(),
    onChangeError: vi.fn(),
    onCreate: vi.fn().mockResolvedValue(false),
    onSaveAsProfile: vi.fn(),
    onCancel: vi.fn(),
  };
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useUIStore.getState().dismissNotice();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  setAuthToken("");
});
const render = async (key = "g_a") => {
  await act(async () => root.render(<ActorConfigModal key={key} {...props} />));
};
const choose = async (value: string) => {
  const select = document.querySelector<HTMLSelectElement>(
    'select[aria-label="workingDirectory"]',
  )!;
  await act(async () => {
    select.value = value;
    select.dispatchEvent(new Event("change", { bubbles: true }));
  });
};
const create = () =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (button) => button.textContent === (props.useProfile ? "createFromProfile" : "addAgent"),
  )!;
it("follows the Group by default and sends a fixed scope independently of runtime Profiles", async () => {
  await render();
  await act(async () => create().click());
  expect(props.onCreate).toHaveBeenLastCalledWith(null, "default", "");
  await choose("s_b");
  const profile: ActorProfile = {
    id: "fixture-profile",
    name: "Fixture",
    runtime: "codex",
    scope: "global",
    runner: "pty",
    command: [],
    submit: "enter",
    env: {},
    created_at: "",
    updated_at: "",
    revision: 1,
  };
  props = {
    ...props,
    useProfile: true,
    profileId: actorProfileIdentityKey(profile),
    actorProfiles: [profile],
  };
  await render();
  await act(async () => create().click());
  expect(props.onCreate).toHaveBeenLastCalledWith(null, "default", "s_b");
});
it("hides the extra choice for a single-scope Group and resets it for another Group or reopening", async () => {
  await render();
  await choose("s_b");
  props = { ...props, scopes: [props.scopes![0]], activeScopeKey: "s_a" };
  await render("g_b");
  expect(document.querySelector('select[aria-label="workingDirectory"]')).toBeNull();
  await act(async () => create().click());
  expect(props.onCreate).toHaveBeenLastCalledWith(null, "default", "");
  props = {
    ...props,
    scopes: [
      { scope_key: "s_a", url: "/a" },
      { scope_key: "s_b", url: "/b" },
    ],
  };
  await render("g_b");
  await choose("s_b");
  props = { ...props, isOpen: false };
  await render("g_b");
  props = { ...props, isOpen: true };
  await render("g_b");
  expect(
    document.querySelector<HTMLSelectElement>('select[aria-label="workingDirectory"]')!.value,
  ).toBe("");
});
it("does not silently retarget a removed selection to the remaining Group directory", async () => {
  await render();
  await choose("s_b");
  props = { ...props, scopes: [props.scopes![0]] };
  await render();
  expect(create().disabled).toBe(true);
  expect(document.body.textContent).toContain("workingDirectoryUnavailable");
  expect(props.onCreate).not.toHaveBeenCalled();
  await choose("");
  expect(create().disabled).toBe(false);
});

const button = (label: string) =>
  [...document.querySelectorAll<HTMLButtonElement>("button")].find(
    (node) => node.textContent === label && !node.closest("[hidden]"),
  );

async function stageFixtureSecret() {
  await act(async () => button("secretManager.addVariable")!.click());
  const panel = host.querySelector<HTMLElement>('[aria-label="secretManager.title"]')!;
  const key = panel.querySelector<HTMLInputElement>("label input")!;
  const value = panel.querySelector<HTMLInputElement>('input[aria-label="secretManager.value"]')!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      key,
      "SYNTHETIC_KEY",
    );
    key.dispatchEvent(new Event("input", { bubbles: true }));
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
      value,
      "synthetic-value",
    );
    value.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("secretManager.addVariable")!.click());
}

it("offers the existing session join flow instead of an unused create form for native sessions", async () => {
  const { isWebAdmin, webReadOnly } = useUIStore.getState();
  try {
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    props = { ...props, groupId: "g_a" };
    await render();
    await act(async () => button("runtimeSwitch.native")!.click());
    expect(button("runtimeSwitch.joinNative")).toBeDefined();
    expect(button("addAgent")).toBeUndefined();
    expect(props.onCreate).not.toHaveBeenCalled();
  } finally {
    useUIStore.setState({ isWebAdmin, webReadOnly });
  }
});

const directorySelect = () =>
  document.querySelector<HTMLSelectElement>('select[aria-label="workingDirectory"]');
const respond = (result: unknown) => new Response(JSON.stringify({ ok: true, result }));
const addedScope = {
  scope_key: "s_c",
  url: "/fixture/new-repository",
  label: "New repository",
  git_remote: "",
};

// Real modal -> AppModals save actions -> HTTP payload; only unrelated modals
// and heavyweight field widgets are stubbed above.
async function editActor(
  overrides: Partial<Actor> = {},
  failNotesOnce = false,
  requests: {
    attach?: () => Promise<Response>;
    list?: () => Promise<Response>;
    prompts?: () => Promise<Response>;
    avatarActor?: (actor: Actor) => Actor;
    resetAvatarActor?: (actor: Actor) => Actor;
    privateKeys?: string[];
    profileSecretFails?: boolean;
  } = {},
) {
  let savedActor = {
    id: "managed",
    title: "Managed Actor",
    runtime: "codex",
    command: [],
    enabled: true,
    running: true,
    role: "peer",
    default_scope_key: "",
    ...overrides,
  } as Actor;
  const profile: ActorProfile = {
    id: "codex-profile",
    name: "Codex Profile",
    runtime: "codex",
    scope: "global",
    runner: "pty",
    command: [],
    submit: "enter",
    env: {},
    created_at: "2026-10-25T00:00:00Z",
    updated_at: "2026-10-25T00:00:00Z",
    revision: 1,
  };
  const mutations: Array<{ path: string; method: string; body: Record<string, unknown> }> = [];
  let createdProfile: ActorProfile | undefined;
  const fetch = vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const path = String(url);
    const method = init?.method || "GET";
    const body =
      typeof init?.body === "string" ? (JSON.parse(init.body) as Record<string, unknown>) : {};
    if (method !== "GET") mutations.push({ path, method, body });
    if (path.endsWith("/prompts")) {
      return requests.prompts?.() || respond({ help: { content: "" } });
    }
    if (path === "/api/v1/fs/recent") {
      return respond({ suggestions: [{ name: "Home", path: "/fixture", icon: "home" }] });
    }
    if (path.startsWith("/api/v1/fs/list?")) {
      if (requests.list) return requests.list();
      return respond({
        path: new URL(path, "https://fixture.invalid").searchParams.get("path"),
        parent: "/fixture",
        items: [{ name: addedScope.label, path: addedScope.url, is_dir: true }],
      });
    }
    if (path === "/api/v1/groups/g_a/attach") {
      return (
        requests.attach?.() || respond({ group_id: "g_a", scope_key: "s_c", scope: addedScope })
      );
    }
    if (path === "/api/v1/profiles?view=accessible")
      return respond({ profiles: [profile, ...(createdProfile ? [createdProfile] : [])] });
    if (path === "/api/v1/actor_profiles" && method === "POST") {
      createdProfile = { ...profile, id: "saved-profile", name: "Saved Profile" };
      return respond({ profile: createdProfile });
    }
    if (path === "/api/v1/actor_profiles/saved-profile/copy_actor_secrets") {
      return respond({ keys: requests.privateKeys || [] });
    }
    if (path === "/api/v1/profiles/saved-profile/env_private" && method === "POST") {
      if (requests.profileSecretFails) {
        return new Response(
          JSON.stringify({ ok: false, error: { code: "fixture", message: "Failed" } }),
        );
      }
      return respond({ keys: ["SYNTHETIC_KEY"] });
    }
    if (path === "/api/v1/local_sessions") {
      return respond({
        sources: [
          {
            source_id: "codex:old",
            native_session_id: "old",
            runtime: "codex",
            title: "Old",
            cwd: "/fixture/a",
            connected: true,
          },
          {
            source_id: "codex:new",
            native_session_id: "new",
            runtime: "codex",
            title: "New",
            cwd: "/fixture/a",
            connected: true,
          },
        ],
      });
    }
    if (path === "/api/v1/groups/g_a/actors" && method === "GET")
      return respond({ actors: [savedActor] });
    if (path === "/api/v1/groups/g_a/actors/managed/avatar" && method === "POST") {
      savedActor = {
        ...savedActor,
        avatar_asset_path: "state/blobs/fixture",
        updated_at: "after-avatar",
      };
      savedActor = requests.avatarActor?.(savedActor) || savedActor;
      return respond({ actor: savedActor });
    }
    if (path === "/api/v1/groups/g_a/actors/managed/avatar?by=user" && method === "DELETE") {
      savedActor = { ...savedActor, avatar_asset_path: "", updated_at: "after-reset" };
      savedActor = requests.resetAvatarActor?.(savedActor) || savedActor;
      return respond({ actor: savedActor });
    }
    if (path === "/api/v1/groups/g_a/actors/managed/runtime_switch" && method === "POST") {
      return respond({
        group_id: "g_a",
        actor_id: "managed",
        op_id: "op",
        switched: true,
        from: { kind: savedActor.external_session_id ? "native" : "managed", runtime: "codex" },
        to: { kind: (body.target as { kind: string }).kind, runtime: "codex" },
        completion: "done",
        start: savedActor.external_session_id ? "started" : "native",
      });
    }
    if (path.endsWith("/env_private?by=user")) return respond({ keys: requests.privateKeys || [] });
    if (path.endsWith("/prompts/help") && method === "PUT") {
      if (failNotesOnce) {
        failNotesOnce = false;
        return new Response(
          JSON.stringify({
            ok: false,
            error: { code: "test_notes", message: "Notes unavailable" },
          }),
        );
      }
      return respond({ content: body.content });
    }
    if (path === "/api/v1/groups/g_a/actors/managed" && method === "POST") {
      savedActor = {
        ...savedActor,
        title: (body.title as string | undefined) ?? savedActor.title,
        default_scope_key:
          (body.default_scope_key as string | undefined) ?? savedActor.default_scope_key,
        profile_id: (body.profile_id as string | undefined) ?? savedActor.profile_id,
      };
      const wireActor = { ...savedActor };
      delete wireActor.avatar_url;
      delete wireActor.has_custom_avatar;
      return respond({ actor: wireActor });
    }
    if (path === "/api/v1/groups/g_a/actors" && method === "POST") {
      return respond({ actor: { id: "created", ...body } });
    }
    if (path.endsWith("/restart?by=user") && method === "POST") return respond({});
    throw new Error(`Unexpected fixture request: ${method} ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  const groupDoc = {
    group_id: "g_a",
    running: true,
    active_scope_key: "s_a",
    scopes: props.scopes,
  };
  useGroupStore.setState({
    selectedGroupId: "g_a",
    groupDoc,
    actors: [savedActor],
    groups: [],
    runtimes: [{ name: "codex", display_name: "Codex", available: true }],
    runtimeDetectionStatus: "ready",
    refreshActors: vi.fn().mockResolvedValue(undefined),
  });
  useFormStore.setState({ editActorNotes: "" });
  useModalStore.getState().openActorEditor(savedActor);
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
        onThemeChange={vi.fn()}
        onTextScaleChange={vi.fn()}
        onDeleteGroup={vi.fn()}
        fetchContext={vi.fn()}
        canManageGroups
      />,
    ),
  );
  return { mutations, groupDoc, profile, fetch, saved: () => savedActor };
}

it("keeps a managed command draft when visiting the Web Model category and returning", async () => {
  await editActor({ command: ["codex", "--model", "example"] });
  expect(useFormStore.getState().editActorCommand).toBe("codex --model example");
  await act(async () => button("runtimeSwitch.web")!.click());
  await act(async () => button("runtimeSwitch.managed")!.click());
  expect(useFormStore.getState().editActorRuntime).toBe("codex");
  expect(useFormStore.getState().editActorCommand).toBe("codex --model example");
});

it.each([false, true])(
  "saves an edited attached directory through the real Actor API without changing the Group (Profile=%s)",
  async (linked) => {
    const { mutations, groupDoc, saved } = await editActor(
      linked ? { profile_id: "codex-profile", profile_scope: "global" } : {},
    );
    expect(directorySelect()?.value).toBe("");
    expect(document.body.textContent).toContain("workingDirectoryEffective: /fixture/a");
    expect(button("common:done")).toBeDefined();
    await choose("s_b");
    expect(document.body.textContent).toContain("workingDirectoryEffective: /fixture/b");
    expect(document.body.textContent).toContain("workingDirectoryEditHint");
    expect(button("common:save")?.disabled).toBe(false);
    await act(async () => button("common:save")!.click());

    expect(mutations).toEqual([
      {
        path: "/api/v1/groups/g_a/actors/managed",
        method: "POST",
        body: {
          by: "user",
          title: "Managed Actor",
          capability_autoload: [],
          default_scope_key: "s_b",
          ...(linked ? { profile_id: "codex-profile", profile_scope: "global" } : {}),
        },
      },
    ]);
    expect(saved().default_scope_key).toBe("s_b");
    expect(useModalStore.getState().editingActor).toBeNull();
    expect(useUIStore.getState().notice?.message).toBe("savedRestartRequired");
    expect(useGroupStore.getState().groupDoc).toBe(groupDoc);
    expect(useGroupStore.getState().selectedGroupId).toBe("g_a");
  },
);

it("keeps the directory draft across Profile switches, cancels it, and saves Follow Group before an explicit restart", async () => {
  const { mutations, profile, saved, groupDoc } = await editActor({ default_scope_key: "s_b" });
  await choose("");
  await act(async () => button("fromActorProfile")!.click());
  const profileSelect = document.querySelector<HTMLSelectElement>(
    'select[aria-label="actorProfile"]',
  )!;
  await act(async () => {
    profileSelect.value = actorProfileIdentityKey(profile);
    profileSelect.dispatchEvent(new Event("change", { bubbles: true }));
  });
  expect(directorySelect()?.value).toBe("");
  await act(async () => button("customAgent")!.click());
  expect(directorySelect()?.value).toBe("");
  await act(async () => button("common:cancel")!.click());
  expect(mutations).toEqual([]);
  await act(async () => useModalStore.getState().openActorEditor(saved()));
  expect(directorySelect()?.value).toBe("s_b");
  await choose("");
  const confirm = vi.fn().mockReturnValueOnce(false).mockReturnValueOnce(true);
  vi.stubGlobal("confirm", confirm);
  await act(async () => button("saveAndRestart")!.click());
  expect(mutations).toEqual([]);
  await act(async () => button("saveAndRestart")!.click());
  expect(confirm).toHaveBeenCalledTimes(2);
  expect(mutations.map(({ path, body }) => [path, body.default_scope_key])).toEqual([
    ["/api/v1/groups/g_a/actors/managed", ""],
    ["/api/v1/groups/g_a/actors/managed/restart?by=user", undefined],
  ]);
  expect(useGroupStore.getState().groupDoc).toBe(groupDoc);
});

it("retains the saved directory after a later notes failure and retries only unfinished work", async () => {
  const { mutations } = await editActor({}, true);
  await choose("s_b");
  const notes = document.querySelector<HTMLTextAreaElement>(
    'textarea[placeholder="actorNotesPlaceholder"]',
  )!;
  await act(async () => {
    const setValue = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
    setValue.call(notes, "Unfinished notes");
    notes.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => button("common:save")!.click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("Notes unavailable");
  expect(directorySelect()?.value).toBe("s_b");
  expect(notes.value).toBe("Unfinished notes");
  expect(button("common:save")?.disabled).toBe(false);
  await act(async () => button("common:save")!.click());
  expect(mutations.map(({ path }) => path)).toEqual([
    "/api/v1/groups/g_a/actors/managed",
    "/api/v1/groups/g_a/prompts/help",
    "/api/v1/groups/g_a/prompts/help",
  ]);
  expect(useModalStore.getState().editingActor).toBeNull();
});

it("blocks a removed edit selection without silently changing it, while native edits never offer a cwd selector", async () => {
  const { mutations, groupDoc, saved } = await editActor();
  await choose("s_b");
  await act(async () =>
    useGroupStore.setState({ groupDoc: { ...groupDoc, scopes: [props.scopes![0]] } }),
  );
  expect(document.body.textContent).toContain("workingDirectoryUnavailable");
  expect(button("common:save")?.disabled).toBe(true);
  expect(mutations).toEqual([]);
  await choose("");
  expect(button("common:done")).toBeDefined();
  await act(async () =>
    useModalStore
      .getState()
      .openActorEditor({ ...saved(), id: "native", external_session_id: "codex:fixture" }),
  );
  expect(directorySelect()).toBeNull();
  expect(document.body.textContent).toContain("nativeSession.configHint");
  expect(mutations).toEqual([]);
});

it.each(["", "s_a", "removed"])(
  "keeps single-scope edits simple without hiding an existing directory choice (%s)",
  async (defaultScopeKey) => {
    props = { ...props, scopes: [props.scopes![0]] };
    const { mutations } = await editActor({ default_scope_key: defaultScopeKey });
    if (!defaultScopeKey) {
      expect(directorySelect()).toBeNull();
    } else {
      expect(directorySelect()).not.toBeNull();
      if (defaultScopeKey === "removed") {
        expect(document.body.textContent).toContain("workingDirectoryUnavailable");
        expect(button("common:save")?.disabled).toBe(true);
      }
      await choose("");
      expect(directorySelect()).toBeNull();
      expect(button("common:save")?.disabled).toBe(false);
    }
    expect(mutations).toEqual([]);
  },
);

async function setDirectoryPath(path: string) {
  const input = document.querySelector<HTMLInputElement>(
    'input[aria-label="workingDirectoryPath"]',
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, path);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function openDirectoryPicker() {
  await act(async () => button("workingDirectoryAdd")!.click());
  expect(button("workingDirectoryUse")).toBeDefined();
}

it.each(["create", "edit"])(
  "can explicitly pin the only directory, then return to Follow Group in %s",
  async (entry) => {
    props = { ...props, scopes: [props.scopes![0]] };
    if (entry === "create") await render();
    else await editActor();
    expect(directorySelect()).toBeNull();
    await act(async () => button("fixedDirectory")!.click());
    expect(directorySelect()?.value).toBe("s_a");
    await choose("");
    expect(directorySelect()).toBeNull();
  },
);

it.each([false, true])(
  "adds a directory in edit without saving Actor/Profile or changing Group selection (Profile=%s)",
  async (linked) => {
    const { mutations, groupDoc, saved } = await editActor(
      linked ? { profile_id: "codex-profile", profile_scope: "global" } : {},
    );
    await act(async () =>
      useFormStore.setState({ createGroupPath: "/keep-create-group", createGroupName: "Keep" }),
    );
    await openDirectoryPicker();
    // The detected repository root in the receipt, not this raw subdirectory,
    // must become the pin and the next-start preview.
    await setDirectoryPath(`${addedScope.url}/subdirectory`);
    await act(async () => button("workingDirectoryUse")!.click());
    expect(mutations).toEqual([
      {
        path: "/api/v1/groups/g_a/attach",
        method: "POST",
        body: { path: `${addedScope.url}/subdirectory`, by: "user", activate: false },
      },
    ]);
    expect(saved().default_scope_key).toBe("");
    expect(directorySelect()?.value).toBe("s_c");
    expect(document.body.textContent).toContain(`workingDirectoryEffective: ${addedScope.url}`);
    expect(document.body.textContent).toContain("workingDirectoryAdded");
    expect(useGroupStore.getState().groupDoc).toEqual({
      ...groupDoc,
      scopes: [...groupDoc.scopes!, addedScope],
    });
    expect(useGroupStore.getState().selectedGroupId).toBe("g_a");
    expect(useFormStore.getState().createGroupPath).toBe("/keep-create-group");
    expect(useFormStore.getState().createGroupName).toBe("Keep");
    await act(async () => button("common:save")!.click());
    expect(mutations.map(({ path }) => path)).toEqual([
      "/api/v1/groups/g_a/attach",
      "/api/v1/groups/g_a/actors/managed",
    ]);
    expect(saved().default_scope_key).toBe("s_c");
    expect(useUIStore.getState().notice?.message).toBe("savedRestartRequired");
  },
);

it.each([false, true])(
  "adds and pins a directory from create only when the Actor is explicitly created (Profile=%s)",
  async (linked) => {
    const { mutations, groupDoc, profile } = await editActor();
    await act(async () => {
      useModalStore.setState({ editingActor: null });
      useFormStore.setState({
        newActorId: "created",
        newActorRuntime: "codex",
        newActorUseProfile: linked,
        newActorProfileId: "",
      });
      useModalStore.getState().openModal("addActor");
    });
    if (linked) {
      const select = document.querySelector<HTMLSelectElement>(
        'select[aria-label="actorProfile"]',
      )!;
      await act(async () => {
        select.value = actorProfileIdentityKey(profile);
        select.dispatchEvent(new Event("change", { bubbles: true }));
      });
    }
    await openDirectoryPicker();
    await setDirectoryPath(addedScope.url);
    await act(async () => button("workingDirectoryUse")!.click());
    expect(mutations).toHaveLength(1);
    expect(mutations[0].body.activate).toBe(false);
    expect(directorySelect()?.value).toBe("s_c");
    expect(useGroupStore.getState().groupDoc?.active_scope_key).toBe(groupDoc.active_scope_key);
    const submit = button(linked ? "createFromProfile" : "addAgent")!;
    expect(submit.disabled).toBe(false);
    await act(async () => submit.click());
    expect(mutations).toHaveLength(2);
    expect(mutations[1].path).toBe("/api/v1/groups/g_a/actors");
    expect(mutations[1].body.default_scope_key).toBe("s_c");
    expect(mutations[1].body.profile_id).toBe(linked ? profile.id : undefined);
    expect(useGroupStore.getState().selectedGroupId).toBe("g_a");
  },
);

it("cancels without attaching and retains a failed path for retry, with explicit duplicate feedback", async () => {
  let attempt = 0;
  const { mutations } = await editActor({}, false, {
    attach: async () => {
      if (attempt++ === 0) {
        return new Response(
          JSON.stringify({
            ok: false,
            error: { code: "scope_conflict", message: "Different worktree" },
          }),
        );
      }
      return respond({ group_id: "g_a", scope_key: "s_c", scope: addedScope });
    },
  });
  await openDirectoryPicker();
  await act(async () => button("common:cancel")!.click());
  expect(mutations).toEqual([]);
  expect(document.body.textContent).toContain("workingDirectoryAddCanceled");
  await openDirectoryPicker();
  await setDirectoryPath(addedScope.url);
  await act(async () => button("workingDirectoryUse")!.click());
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("Different worktree");
  expect(
    document.querySelector<HTMLInputElement>('input[aria-label="workingDirectoryPath"]')?.value,
  ).toBe(addedScope.url);
  expect(button("workingDirectoryUse")?.disabled).toBe(false);
  expect(directorySelect()?.value).toBe("");
  await act(async () => button("workingDirectoryUse")!.click());
  expect(directorySelect()?.value).toBe("s_c");
  await openDirectoryPicker();
  await act(async () => button("workingDirectoryUse")!.click());
  expect(document.body.textContent).toContain("workingDirectoryAlreadyAttached");
  expect(useGroupStore.getState().groupDoc?.scopes).toHaveLength(3);
  expect(mutations.every(({ path }) => path.endsWith("/attach"))).toBe(true);
});

function deferredResponse() {
  let resolve!: (response: Response) => void;
  const promise = new Promise<Response>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

it.each(["actor", "group", "modal", "cancel"])(
  "ignores a late attach response after changing %s",
  async (change) => {
    const pending = deferredResponse();
    const { mutations, groupDoc, saved } = await editActor({}, false, {
      attach: () => pending.promise,
    });
    await choose("s_b");
    await openDirectoryPicker();
    await setDirectoryPath(addedScope.url);
    await act(async () => button("workingDirectoryUse")!.click());
    expect(button("common:save")?.disabled).toBe(true);
    expect(mutations).toHaveLength(1);
    if (change === "actor") {
      await act(async () =>
        useModalStore
          .getState()
          .openActorEditor({ ...saved(), id: "other", default_scope_key: "s_a" }),
      );
    } else if (change === "group") {
      await act(async () =>
        useGroupStore.setState({
          selectedGroupId: "g_b",
          groupDoc: { ...groupDoc, group_id: "g_b" },
        }),
      );
    } else if (change === "modal") {
      await act(async () => useModalStore.setState({ editingActor: null }));
      await act(async () => useModalStore.getState().openActorEditor(saved()));
    } else {
      await act(async () => button("common:cancel")!.click());
      expect(document.body.textContent).toContain("workingDirectoryAddStopped");
    }
    await act(async () =>
      pending.resolve(respond({ group_id: "g_a", scope_key: "s_c", scope: addedScope })),
    );
    expect(directorySelect()?.value).toBe(
      change === "actor" ? "s_a" : change === "cancel" ? "s_b" : "",
    );
    expect(useGroupStore.getState().groupDoc?.scopes).toHaveLength(2);
    expect(mutations).toHaveLength(1);
    expect(saved().default_scope_key).toBe("");
    expect(useGroupStore.getState().selectedGroupId).toBe(change === "group" ? "g_b" : "g_a");
  },
);

it("ignores a late directory listing after an Actor change", async () => {
  const pending = deferredResponse();
  const { saved, mutations } = await editActor({}, false, { list: () => pending.promise });
  await openDirectoryPicker();
  await act(async () => useModalStore.getState().openActorEditor({ ...saved(), id: "other" }));
  await act(async () =>
    pending.resolve(respond({ path: "/old-actor-directory", parent: "/", items: [] })),
  );
  expect(document.querySelector('input[aria-label="workingDirectoryPath"]')).toBeNull();
  expect(document.body.textContent).not.toContain("/old-actor-directory");
  expect(mutations).toEqual([]);
});

it.each(["create", "edit"])(
  "waits for the selected Group's document before allowing %s to add directories",
  async (entry) => {
    const { groupDoc, mutations } = await editActor();
    await act(async () => {
      if (entry === "create") {
        useModalStore.setState({ editingActor: null });
        useModalStore.getState().openModal("addActor");
      }
      useGroupStore.setState({ selectedGroupId: "g_b" });
    });
    expect(button("workingDirectoryAdd")).toBeUndefined();
    expect(mutations).toEqual([]);
    await act(async () => useGroupStore.setState({ groupDoc: { ...groupDoc, group_id: "g_b" } }));
    expect(button("workingDirectoryAdd")).toBeDefined();
  },
);

it.each(["group", "modal"])(
  "ignores a late create-form attachment after changing %s",
  async (change) => {
    const pending = deferredResponse();
    const { groupDoc, mutations } = await editActor({}, false, { attach: () => pending.promise });
    await act(async () => {
      useModalStore.setState({ editingActor: null });
      useFormStore.setState({ newActorId: "created", newActorRuntime: "codex" });
      useModalStore.getState().openModal("addActor");
    });
    await openDirectoryPicker();
    await setDirectoryPath(addedScope.url);
    await act(async () => button("workingDirectoryUse")!.click());
    expect(button("addAgent")?.disabled).toBe(true);
    await act(async () => {
      if (change === "group") {
        useGroupStore.setState({
          selectedGroupId: "g_b",
          groupDoc: { ...groupDoc, group_id: "g_b" },
        });
      } else {
        useModalStore.getState().closeModal("addActor");
      }
    });
    if (change === "modal") {
      await act(async () => useModalStore.getState().openModal("addActor"));
    }
    await act(async () =>
      pending.resolve(respond({ group_id: "g_a", scope_key: "s_c", scope: addedScope })),
    );
    expect(directorySelect()?.value).toBe("");
    expect(useGroupStore.getState().groupDoc?.scopes).toHaveLength(2);
    expect(mutations).toHaveLength(1);
    expect(mutations[0].path).toBe("/api/v1/groups/g_a/attach");
  },
);

it("keeps an attached Group directory after canceling the Actor without saving its draft pin", async () => {
  const { mutations, saved } = await editActor();
  await openDirectoryPicker();
  await setDirectoryPath(addedScope.url);
  await act(async () => button("workingDirectoryUse")!.click());
  expect(directorySelect()?.value).toBe("s_c");
  await act(async () => button("common:cancel")!.click());
  expect(mutations.map(({ path }) => path)).toEqual(["/api/v1/groups/g_a/attach"]);
  expect(saved().default_scope_key).toBe("");
  expect(useGroupStore.getState().groupDoc?.scopes).toHaveLength(3);
  await act(async () => useModalStore.getState().openActorEditor(saved()));
  expect(directorySelect()?.value).toBe("");
  expect(directorySelect()?.querySelector('option[value="s_c"]')).not.toBeNull();
});

it.each(["native", "managed"] as const)(
  "uses the successful avatar update's version for a subsequent %s switch",
  async (kind) => {
    const { isWebAdmin, webReadOnly } = useUIStore.getState();
    try {
      useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
      vi.spyOn(switchState, "refreshRuntimeSwitchGroup").mockResolvedValue(undefined);
      const { mutations } = await editActor({
        generation: "generation-1",
        updated_at: "before-avatar",
        ...(kind === "native" ? { external_session_id: "codex:old" } : {}),
      });
      await act(async () => button("fixture avatar upload")!.click());
      expect(useModalStore.getState().editingActor?.updated_at).toBe("after-avatar");
      await act(async () =>
        button(kind === "native" ? "runtimeSwitch.managed" : "runtimeSwitch.native")!.click(),
      );
      if (kind === "managed") {
        const select = document.querySelector<HTMLSelectElement>(
          'select[aria-label="runtimeSwitch.source"]',
        )!;
        await act(async () => {
          select.value = "codex:new";
          select.dispatchEvent(new Event("change", { bubbles: true }));
        });
      }
      await act(async () => button("runtimeSwitch.switch")!.click());
      await act(async () => button("runtimeSwitch.confirm")!.click());
      const call = mutations.find((item) => item.path.endsWith("/runtime_switch"));
      expect(call?.body.expected_actor_generation).toBe("generation-1");
      expect(call?.body.expected_actor_updated_at).toBe("after-avatar");
      if (kind === "native") expect(call?.body.expected_source_id).toBe("codex:old");
      else expect(call?.body.target).toMatchObject({ kind: "native", source_id: "codex:new" });
    } finally {
      useUIStore.setState({ isWebAdmin, webReadOnly });
    }
  },
);

it("does not silently rebase an editor if the native session changed during avatar upload", async () => {
  const { isWebAdmin, webReadOnly } = useUIStore.getState();
  try {
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    const { mutations } = await editActor(
      { generation: "generation-1", updated_at: "before-avatar", external_session_id: "codex:old" },
      false,
      { avatarActor: (actor) => ({ ...actor, external_session_id: "codex:other" }) },
    );
    await act(async () => button("fixture avatar upload")!.click());
    expect(host.textContent).toContain("runtimeSwitch.conflict");
    expect(host.textContent).toContain("avatarSaved");
    expect(useModalStore.getState().editingActor?.updated_at).toBe("before-avatar");
    expect(mutations.some((item) => item.path.endsWith("/runtime_switch"))).toBe(false);
  } finally {
    useUIStore.setState({ isWebAdmin, webReadOnly });
  }
});

it("reports a successful avatar reset even if the Actor changed concurrently", async () => {
  await editActor(
    {
      generation: "generation-1",
      updated_at: "before-avatar",
      external_session_id: "codex:old",
      avatar_asset_path: "state/blobs/fixture",
      avatar_url: "/api/v1/groups/g_a/blobs/fixture",
      has_custom_avatar: true,
    },
    false,
    { resetAvatarActor: (actor) => ({ ...actor, external_session_id: "codex:other" }) },
  );
  await act(async () => button("fixture avatar reset")!.click());
  expect(host.textContent).toContain("avatarReset");
  expect(host.textContent).toContain("runtimeSwitch.conflict");
  expect(useModalStore.getState().editingActor?.updated_at).toBe("before-avatar");
});

it("saves only native metadata even after selecting a managed switch draft", async () => {
  const { isWebAdmin, webReadOnly } = useUIStore.getState();
  try {
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    const { mutations } = await editActor({
      generation: "generation-1",
      updated_at: "before",
      external_session_id: "codex:old",
      capability_autoload: ["existing-capability"],
    });
    await act(async () => button("runtimeSwitch.managed")!.click());
    await act(async () =>
      useFormStore.setState({ editActorTitle: "Updated title", editActorNotes: "New notes" }),
    );
    await act(async () => button("common:save")!.click());
    const actorUpdate = mutations.find((item) => item.path === "/api/v1/groups/g_a/actors/managed");
    expect(actorUpdate?.body).toEqual({ by: "user", title: "Updated title" });
    expect(mutations.some((item) => item.path.endsWith("/prompts/help"))).toBe(true);
    expect(mutations.some((item) => item.path.endsWith("/runtime_switch"))).toBe(false);
  } finally {
    useUIStore.setState({ isWebAdmin, webReadOnly });
  }
});

it.each([
  { env: { SERVICE_URL: "https://updated.invalid" } },
  { capability_autoload: ["new-capability"] },
])(
  "does not rebase an avatar update over a concurrent managed configuration change %#",
  async (patch) => {
    const { isWebAdmin, webReadOnly } = useUIStore.getState();
    try {
      useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
      const { mutations } = await editActor(
        { generation: "generation-1", updated_at: "before-avatar" },
        false,
        { avatarActor: (actor) => ({ ...actor, ...patch }) },
      );
      await act(async () => button("fixture avatar upload")!.click());
      expect(host.textContent).toContain("runtimeSwitch.conflict");
      expect(useModalStore.getState().editingActor?.updated_at).toBe("before-avatar");
      expect(mutations.some((item) => item.path.endsWith("/runtime_switch"))).toBe(false);
    } finally {
      useUIStore.setState({ isWebAdmin, webReadOnly });
    }
  },
);

it("shows the committed avatar and reset immediately even if the actor list has not refreshed", async () => {
  const { isWebAdmin, webReadOnly } = useUIStore.getState();
  try {
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    await editActor({ generation: "generation-1", updated_at: "before-avatar" });
    await act(async () => button("fixture avatar upload")!.click());
    expect(host.querySelector("[data-avatar-url]")?.getAttribute("data-avatar-url")).toBe(
      "/api/v1/groups/g_a/blobs/fixture",
    );
    expect(button("fixture avatar reset")!.disabled).toBe(false);
    await act(async () => button("fixture avatar reset")!.click());
    expect(host.querySelector("[data-avatar-url]")?.getAttribute("data-avatar-url")).toBe("");
    expect(button("fixture avatar reset")!.disabled).toBe(true);
  } finally {
    useUIStore.setState({ isWebAdmin, webReadOnly });
  }
});

it.each(["response", "network"] as const)(
  "does not overwrite native notes after a %s load failure and restores them on retry",
  async (failure) => {
    const { isWebAdmin, webReadOnly } = useUIStore.getState();
    let attempts = 0;
    try {
      useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
      const { mutations } = await editActor(
        { generation: "generation-1", updated_at: "before", external_session_id: "codex:old" },
        false,
        {
          prompts: async () => {
            attempts += 1;
            if (attempts === 1) {
              if (failure === "network") throw new Error("synthetic network error");
              return new Response(
                JSON.stringify({ ok: false, error: { code: "fixture", message: "Unavailable" } }),
              );
            }
            return respond({
              help: { content: updateActorHelpNote("", "managed", "Original notes", ["managed"]) },
            });
          },
        },
      );
      expect(host.textContent).toContain("actorNotesLoadFailed");
      expect(host.querySelector<HTMLTextAreaElement>("textarea")?.disabled).toBe(true);
      expect(button("common:done")!.disabled).toBe(true);
      expect(mutations.some((item) => item.path.endsWith("/prompts/help"))).toBe(false);
      await act(async () => button("common:retry")!.click());
      expect(host.querySelector<HTMLTextAreaElement>("textarea")?.value).toBe("Original notes");
      expect(host.querySelector<HTMLTextAreaElement>("textarea")?.disabled).toBe(false);
      await act(async () => useFormStore.setState({ editActorNotes: "Updated notes" }));
      await act(async () => button("common:save")!.click());
      const noteSave = mutations.find((item) => item.path.endsWith("/prompts/help"));
      expect(String(noteSave?.body.content)).toContain("Updated notes");
    } finally {
      useUIStore.setState({ isWebAdmin, webReadOnly });
    }
  },
);

it("keeps separate create drafts when moving among managed, Web Model, and native categories", async () => {
  const { isWebAdmin, webReadOnly } = useUIStore.getState();
  const managedProfile: ActorProfile = {
    id: "managed-profile",
    name: "Managed profile",
    runtime: "codex",
    scope: "global",
    runner: "pty",
    command: ["codex"],
    submit: "enter",
    env: {},
    created_at: "",
    updated_at: "",
    revision: 1,
  };
  const webProfile: ActorProfile = {
    ...managedProfile,
    id: "web-profile",
    name: "Web profile",
    runtime: "web_model",
  };
  function CreateDraft() {
    const [runtime, setRuntime] = useState<CreateActorConfigProps["runtime"]>("codex");
    const [command, setCommand] = useState("codex --fixture");
    const [useDefault, setUseDefault] = useState(false);
    const [useProfile, setUseProfile] = useState(true);
    const [profileId, setProfileId] = useState(actorProfileIdentityKey(managedProfile));
    return (
      <ActorConfigModal
        {...props}
        groupId="g_a"
        actorProfiles={[managedProfile, webProfile]}
        runtime={runtime}
        onChangeRuntime={setRuntime}
        command={command}
        onChangeCommand={setCommand}
        useDefaultCommand={useDefault}
        onChangeUseDefaultCommand={setUseDefault}
        useProfile={useProfile}
        onChangeUseProfile={setUseProfile}
        profileId={profileId}
        onChangeProfileId={setProfileId}
      />
    );
  }
  try {
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    await act(async () => root.render(<CreateDraft />));
    expect(
      document.querySelector<HTMLSelectElement>('select[aria-label="actorProfile"]')?.value,
    ).toBe(actorProfileIdentityKey(managedProfile));
    await act(async () => button("runtimeSwitch.web")!.click());
    await act(async () => button("fromActorProfile")!.click());
    const select = document.querySelector<HTMLSelectElement>('select[aria-label="actorProfile"]')!;
    await act(async () => {
      select.value = actorProfileIdentityKey(webProfile);
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => button("runtimeSwitch.native")!.click());
    await act(async () => button("runtimeSwitch.managed")!.click());
    expect(
      document.querySelector<HTMLSelectElement>('select[aria-label="actorProfile"]')?.value,
    ).toBe(actorProfileIdentityKey(managedProfile));
    await act(async () => button("runtimeSwitch.web")!.click());
    expect(
      document.querySelector<HTMLSelectElement>('select[aria-label="actorProfile"]')?.value,
    ).toBe(actorProfileIdentityKey(webProfile));
  } finally {
    useUIStore.setState({ isWebAdmin, webReadOnly });
  }
});

it.each([
  { useProfile: true, runtime: "codex" as const },
  { useProfile: false, runtime: "web_model" as const },
])("does not drop a private environment draft while creating with $runtime", async (target) => {
  props = { ...props, ...target, secretsSetText: "SYNTHETIC_KEY=synthetic-value" };
  await render();
  await act(async () => create().click());
  expect(props.onCreate).not.toHaveBeenCalled();
  expect(props.onChangeError).toHaveBeenCalledWith("secretDraftNotApplied");
});

it("does not discard an in-progress or staged private environment edit when changing category", async () => {
  const { isWebAdmin, webReadOnly } = useUIStore.getState();
  try {
    useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
    const { mutations } = await editActor({ generation: "generation-1", updated_at: "before" });
    await act(async () => button("secretManager.addVariable")!.click());
    await act(async () => button("saveAndRestart")!.click());
    expect(host.textContent).toContain("secretEditorIncomplete");
    expect(mutations).toHaveLength(0);
    await act(async () => button("runtimeSwitch.web")!.click());
    expect(host.textContent).toContain("secretEditorIncomplete");
    expect(button("runtimeSwitch.managed")!.getAttribute("aria-pressed")).toBe("true");

    await act(async () =>
      host
        .querySelector<HTMLButtonElement>(
          '[aria-label="secretManager.title"] [aria-label="common:cancel"]',
        )!
        .click(),
    );
    await stageFixtureSecret();
    await act(async () => button("runtimeSwitch.web")!.click());
    await act(async () => button("common:save")!.click());
    expect(host.textContent).toContain("secretDraftNotApplied");
    expect(mutations.some((item) => item.path === "/api/v1/groups/g_a/actors/managed")).toBe(false);
    await act(async () => button("runtimeSwitch.managed")!.click());
    expect(host.textContent).toContain("SYNTHETIC_KEY");
  } finally {
    useUIStore.setState({ isWebAdmin, webReadOnly });
  }
});

it("keeps an existing private-value edit until it is finished or canceled", async () => {
  await editActor({}, false, { privateKeys: ["KEY_A", "KEY_B"] });
  const edit = (key: string) =>
    host.querySelector<HTMLButtonElement>(`button[aria-label="secretManager.update ${key}"]`)!;
  await act(async () => edit("KEY_A").click());
  const input = host.querySelector<HTMLInputElement>(
    'input[aria-label="secretManager.newValue KEY_A"]',
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, "draft");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(edit("KEY_A").disabled).toBe(true);
  expect(edit("KEY_B").disabled).toBe(true);
  expect(input.value).toBe("draft");
});

it("accepts an avatar update for a foreman when the mutation response omits the effective role", async () => {
  await editActor(
    { generation: "generation-1", updated_at: "before-avatar", role: "foreman" },
    false,
    { avatarActor: (actor) => ({ ...actor, role: undefined }) },
  );
  await act(async () => button("fixture avatar upload")!.click());
  expect(host.textContent).not.toContain("runtimeSwitch.conflict");
  expect(host.querySelector("[data-avatar-url]")?.getAttribute("data-avatar-url")).toBe(
    "/api/v1/groups/g_a/blobs/fixture",
  );
  expect(useModalStore.getState().editingActor?.role).toBe("foreman");
  expect(useModalStore.getState().editingActor?.updated_at).toBe("after-avatar");
});

it("keeps the avatar visible when a native member's title saves but notes fail", async () => {
  await editActor(
    {
      external_session_id: "codex:old",
      avatar_asset_path: "state/blobs/fixture",
      avatar_url: "/api/v1/groups/g_a/blobs/fixture",
      has_custom_avatar: true,
    },
    true,
  );
  await act(async () =>
    useFormStore.setState({ editActorTitle: "Changed title", editActorNotes: "New notes" }),
  );
  await act(async () => button("common:save")!.click());
  expect(host.textContent).toContain("Notes unavailable");
  expect(host.querySelector("[data-avatar-url]")?.getAttribute("data-avatar-url")).toBe(
    "/api/v1/groups/g_a/blobs/fixture",
  );
  expect(button("fixture avatar reset")!.disabled).toBe(false);
});

it("refuses a Profile whose runtime has moved out of the selected create category", async () => {
  const movedProfile: ActorProfile = {
    id: "moved-profile",
    name: "Moved profile",
    scope: "global",
    runtime: "web_model",
    runner: "pty",
    command: [],
    submit: "enter",
    env: {},
    created_at: "",
    updated_at: "",
    revision: 2,
  };
  props = {
    ...props,
    useProfile: true,
    profileId: actorProfileIdentityKey(movedProfile),
    actorProfiles: [movedProfile],
  };
  await render();
  await act(async () => create().click());
  expect(props.onCreate).not.toHaveBeenCalled();
  expect(props.onChangeError).toHaveBeenCalledWith("profileRequired");
});

it("does not lose a secret draft when returning to the original Profile", async () => {
  const { mutations } = await editActor({ profile_id: "codex-profile", profile_scope: "global" });
  await act(async () => button("customAgent")!.click());
  await stageFixtureSecret();
  await act(async () => button("fromActorProfile")!.click());
  expect(button("common:save")).toBeDefined();
  await act(async () => button("common:save")!.click());
  expect(host.textContent).toContain("secretDraftNotApplied");
  expect(mutations.some((item) => item.path === "/api/v1/groups/g_a/actors/managed")).toBe(false);
});

it.each(["use", "keep", "fail"] as const)(
  "handles a staged secret when saving a Profile (%s)",
  async (outcome) => {
    vi.stubGlobal("prompt", vi.fn().mockReturnValue("Saved Profile"));
    vi.stubGlobal("confirm", vi.fn().mockReturnValue(outcome === "use"));
    const { mutations } = await editActor({}, false, { profileSecretFails: outcome === "fail" });
    await stageFixtureSecret();
    await act(async () => button("profileToolsSection")!.click());
    await act(async () => button("addToActorProfiles")!.click());
    const secretWrite = mutations.find(
      (item) => item.path === "/api/v1/profiles/saved-profile/env_private",
    );
    expect(secretWrite?.body.set).toEqual({ SYNTHETIC_KEY: "synthetic-value" });
    if (outcome === "use") {
      await act(async () => button("common:save")!.click());
      expect(host.textContent).not.toContain("secretDraftNotApplied");
      expect(
        mutations.find((item) => item.path === "/api/v1/groups/g_a/actors/managed")?.body,
      ).toMatchObject({ profile_id: "saved-profile" });
    } else {
      await act(async () => button("secretsSection")!.click());
      expect(host.textContent).toContain("SYNTHETIC_KEY");
    }
  },
);

it.each(["common:save", "saveAndRestart"])(
  "asks before copying Profile secrets when %s moves to a Web Model",
  async (saveLabel) => {
    const { isWebAdmin, webReadOnly } = useUIStore.getState();
    try {
      useUIStore.setState({ isWebAdmin: true, webReadOnly: false });
      const confirm = vi.fn().mockReturnValue(false);
      vi.stubGlobal("confirm", confirm);
      const { mutations } = await editActor({
        generation: "generation-1",
        updated_at: "before",
        profile_id: "codex-profile",
        profile_scope: "global",
      });
      await act(async () => button("runtimeSwitch.web")!.click());
      await act(async () => button(saveLabel)!.click());
      expect(confirm).toHaveBeenCalledOnce();
      expect(confirm.mock.calls[0][0]).toContain("profileToWebSecretsConfirm");
      expect(mutations.some((item) => item.path === "/api/v1/groups/g_a/actors/managed")).toBe(
        false,
      );
    } finally {
      useUIStore.setState({ isWebAdmin, webReadOnly });
    }
  },
);
