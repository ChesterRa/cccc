// @vitest-environment happy-dom
import { act, createRef } from "react";
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
vi.mock("../ActorAvatarField", () => ({ ActorAvatarField: () => null }));
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
  props = { ...props, useProfile: true };
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
    (node) => node.textContent === label,
  );
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
  requests: { attach?: () => Promise<Response>; list?: () => Promise<Response> } = {},
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
  const fetch = vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const path = String(url);
    const method = init?.method || "GET";
    const body = JSON.parse(String(init?.body || "{}")) as Record<string, unknown>;
    if (method !== "GET") mutations.push({ path, method, body });
    if (path.endsWith("/prompts")) {
      return respond({ help: { content: "" } });
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
    if (path === "/api/v1/profiles?view=accessible") return respond({ profiles: [profile] });
    if (path.endsWith("/env_private?by=user")) return respond({ keys: [] });
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
        default_scope_key:
          (body.default_scope_key as string | undefined) ?? savedActor.default_scope_key,
        profile_id: (body.profile_id as string | undefined) ?? savedActor.profile_id,
      };
      return respond({ actor: savedActor });
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
