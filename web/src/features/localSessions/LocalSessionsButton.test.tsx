// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, GroupDoc, LocalSessionSource } from "../../types";
import { useGroupStore, useUIStore } from "../../stores";
import { setAuthToken } from "../../services/api/base";
import enLayout from "../../i18n/locales/en/layout.json";
import enActors from "../../i18n/locales/en/actors.json";
import enCommon from "../../i18n/locales/en/common.json";
import zhLayout from "../../i18n/locales/zh/layout.json";
import zhActors from "../../i18n/locales/zh/actors.json";
import zhCommon from "../../i18n/locales/zh/common.json";
import jaLayout from "../../i18n/locales/ja/layout.json";
import jaActors from "../../i18n/locales/ja/actors.json";
import jaCommon from "../../i18n/locales/ja/common.json";

const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  create: vi.fn(),
  join: vi.fn(),
  members: vi.fn(),
  resume: vi.fn(),
  refresh: vi.fn(),
  refreshActors: vi.fn(),
  error: vi.fn(),
}));
vi.mock("../../services/api/localSessions", () => ({
  fetchLocalSessions: mocks.fetch,
  createLocalConversation: mocks.create,
  joinLocalSessions: mocks.join,
  resumeLocalSessionAccess: mocks.resume,
}));
vi.mock("../../services/api/actors", async (original) => ({
  ...(await original<typeof import("../../services/api/actors")>()),
  fetchActors: mocks.members,
}));
vi.mock("../../components/ActorAvatar", () => ({ ActorAvatar: () => <span /> }));
import { LocalSessionsButton } from "./LocalSessionsButton";

const i18n = createInstance();
let host: HTMLDivElement;
let root: Root;
const source: LocalSessionSource = {
  source_id: "codex:native",
  runtime: "codex",
  native_session_id: "native",
  title: "Codex · workspace",
  cwd: "/synthetic/project",
  connected: true,
};
const second: LocalSessionSource = {
  source_id: "claude:second",
  runtime: "claude",
  native_session_id: "second",
  title: "Claude · other workspace",
  cwd: "/synthetic/other",
  connected: true,
};
const existingGroup: GroupDoc = {
  group_id: "g-existing",
  title: "Existing Group",
  state: "paused",
  running: false,
  runtime_status: {
    lifecycle_state: "paused",
    runtime_running: false,
    running_actor_count: 0,
    has_running_foreman: false,
  },
  active_scope_key: "original-scope",
  scopes: [{ scope_key: "original-scope", url: "/synthetic/original" }],
};

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.resetAllMocks();
  setAuthToken("");
  useGroupStore.setState({
    ...useGroupStore.getInitialState(),
    groups: [existingGroup, { group_id: "g-untitled" }],
    selectedGroupId: existingGroup.group_id,
    groupDoc: existingGroup,
    refreshGroups: mocks.refresh,
    refreshActors: mocks.refreshActors,
  });
  useUIStore.setState({ showError: mocks.error });
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      fallbackLng: "en",
      defaultNS: "layout",
      resources: {
        en: { layout: enLayout, actors: enActors, common: enCommon },
        zh: { layout: zhLayout, actors: zhActors, common: zhCommon },
        ja: { layout: jaLayout, actors: jaActors, common: jaCommon },
      },
      interpolation: { escapeValue: false },
    });
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source] } });
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [] } });
  mocks.resume.mockResolvedValue({ ok: true, result: { actor: { enabled: true } } });
  mocks.create.mockResolvedValue({ ok: true, result: { group_id: "g-created", reused: false } });
  mocks.join.mockResolvedValue({
    ok: true,
    result: { group_id: existingGroup.group_id, group: existingGroup, reused: false },
  });
  mocks.refresh.mockResolvedValue(undefined);
  mocks.refreshActors.mockResolvedValue(undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useGroupStore.setState(useGroupStore.getInitialState(), true);
  useUIStore.setState(useUIStore.getInitialState(), true);
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function click(element: Element | null) {
  expect(element).not.toBeNull();
  await act(async () => {
    (element as HTMLElement).click();
  });
}
function button(label: string) {
  return Array.from(document.body.querySelectorAll("button")).find((b) => b.textContent === label)!;
}
function row(title: string) {
  return Array.from(document.body.querySelectorAll("label")).find((label) =>
    label.textContent?.includes(title),
  )!;
}
function checkbox(title: string) {
  return row(title).querySelector<HTMLInputElement>('input[type="checkbox"]')!;
}
function target() {
  return document.querySelector<HTMLButtonElement>('button[aria-haspopup="menu"]')!;
}
function foreman() {
  return document.querySelector<HTMLButtonElement>('section button[aria-haspopup="menu"]');
}
async function chooseForeman(sourceId: string) {
  await click(foreman());
  await click(
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
      (option) => option.dataset.value === sourceId,
    )!,
  );
}
async function chooseTarget(groupId: string) {
  await click(target());
  await click(
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).find(
      (option) => option.dataset.value === groupId,
    )!,
  );
}
async function mount(select = vi.fn(), collapsed = false) {
  await act(async () =>
    root.render(
      <I18nextProvider i18n={i18n}>
        <LocalSessionsButton collapsed={collapsed} isDark={false} onSelectGroup={select} />
      </I18nextProvider>,
    ),
  );
}
async function open(select = vi.fn()) {
  await mount(select);
  await click(host.querySelector("button"));
}

it("discovers only when opened and creates a new Group with a separate selection count", async () => {
  const select = vi.fn();
  await mount(select);
  expect(mocks.fetch).not.toHaveBeenCalled();
  await click(button("Connect existing sessions"));
  expect(host.querySelector('[role="dialog"]')).toBeNull();
  expect(document.querySelector('[role="dialog"]')?.getAttribute("aria-labelledby")).toBe(
    "local-sessions-title",
  );
  expect(document.getElementById("local-sessions-title")?.textContent).toBe(
    "Connect existing sessions",
  );
  expect(document.body.textContent).toContain(source.title);
  expect(document.body.textContent).toContain(source.cwd);
  expect(target().getAttribute("aria-label")).toBe("Group");
  expect(target().dataset.value).toBe("");
  expect(target().textContent).toContain("+ New Group");
  expect(document.body.textContent).toContain("Selected sessions: 0 / 16");
  expect(button("Create & connect").disabled).toBe(true);
  await click(checkbox(source.title));
  expect(document.body.textContent).toContain("Selected sessions: 1 / 16");
  await click(button("Create & connect"));
  expect(mocks.create).toHaveBeenCalledWith([source.source_id]);
  expect(mocks.join).not.toHaveBeenCalled();
  expect(mocks.refresh).toHaveBeenCalledOnce();
  expect(mocks.refreshActors).not.toHaveBeenCalled();
  expect(select).toHaveBeenCalledWith("g-created");
});

it("announces discovery loading instead of claiming that the session directory is empty", async () => {
  let finish!: (value: unknown) => void;
  mocks.fetch.mockReturnValue(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await open();
  expect(document.body.textContent).toContain("Loading...");
  expect(document.querySelector('[role="dialog"] [aria-busy="true"]')).not.toBeNull();
  expect(document.body.textContent).not.toContain(enLayout.localSessions.empty);
  expect(button("Create & connect").disabled).toBe(true);
  await click(button("Create & connect"));
  expect(mocks.create).not.toHaveBeenCalled();
  await act(async () => finish({ ok: true, result: { sources: [source] } }));
  expect(document.body.textContent).not.toContain("Loading...");
  expect(checkbox(source.title).disabled).toBe(false);
});

it("explains an empty directory without creating a Group or enabling an empty submission", async () => {
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [] } });
  await open();
  expect(document.body.textContent).toContain(enLayout.localSessions.empty);
  // With nothing connected yet, the setup steps are what the reader needs.
  const [setupDetails, troubleshooting] = Array.from(document.querySelectorAll("details"));
  expect(setupDetails?.open).toBe(true);
  expect(troubleshooting?.open).toBe(false);
  expect(document.body.textContent).not.toContain(enLayout.localSessions.unavailableHint);
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(document.body.textContent).toContain("Selected sessions: 0 / 16");
  expect(button("Create & connect").disabled).toBe(true);
  expect(mocks.create).not.toHaveBeenCalled();
});

it("shows first-selected defaults only for a new Group", async () => {
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
  await open();
  expect(document.body.textContent).toContain("Create a new Group for the selected sessions.");
  await click(checkbox(second.title));
  await click(checkbox(source.title));
  expect(document.body.textContent).toContain("Selected sessions: 2 / 16");
  expect(button("Create & connect").disabled).toBe(false);
  const defaults = document.querySelector('section[aria-label="New Group defaults"]')!;
  expect(defaults.textContent).toContain("Foreman");
  expect(defaults.textContent).toContain(second.title);
  expect(defaults.textContent).toContain("Working directory");
  expect(defaults.textContent).toContain(second.cwd);
  expect(defaults.textContent).not.toContain(source.cwd);
  await click(checkbox(second.title));
  expect(defaults.textContent).toContain(source.title);
  expect(defaults.textContent).toContain(source.cwd);
  expect(button("Create & connect").disabled).toBe(false);
});

it("submits the explicitly chosen Foreman first, keeping the other selections and their relative order", async () => {
  vi.useFakeTimers();
  const third = {
    ...source,
    source_id: "codex:third",
    title: "Third workspace",
    cwd: "/synthetic/third",
  };
  const unselected = { ...source, source_id: "codex:unselected", title: "Unselected workspace" };
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: { sources: [source, second, third, unselected] },
  });
  await open();
  await click(checkbox(second.title));
  expect(foreman()).toBeNull();
  await click(checkbox(source.title));
  await click(checkbox(third.title));
  expect(foreman()?.dataset.value).toBe(second.source_id);
  await click(foreman());
  expect(
    Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]')).map(
      (option) => option.dataset.value,
    ),
  ).toEqual([second.source_id, source.source_id, third.source_id]);
  await click(document.querySelector(`[role="menuitemradio"][data-value="${third.source_id}"]`));
  const defaults = document.querySelector('section[aria-label="New Group defaults"]')!;
  expect(foreman()?.dataset.value).toBe(third.source_id);
  expect(defaults.textContent).toContain(third.cwd);
  expect(defaults.textContent).not.toContain(second.cwd);
  expect(defaults.textContent).toContain(enLayout.localSessions.foremanDirectoryHint);
  expect([source, second, third].every((item) => checkbox(item.title).checked)).toBe(true);
  expect(checkbox(unselected.title).checked).toBe(false);
  expect(mocks.create).not.toHaveBeenCalled();
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: { sources: [unselected, third, source, second] },
  });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(foreman()?.dataset.value).toBe(third.source_id);
  await click(button("Create & connect"));
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith([
    third.source_id,
    second.source_id,
    source.source_id,
  ]);
  expect(mocks.join).not.toHaveBeenCalled();
});

it("retains an unavailable chosen Foreman until explicitly deselected, then uses the remaining session", async () => {
  vi.useFakeTimers();
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
  await open();
  await click(checkbox(source.title));
  await click(checkbox(second.title));
  await chooseForeman(second.source_id);
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: { sources: [source, { ...second, connected: false }] },
  });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(foreman()?.dataset.value).toBe(second.source_id);
  expect(document.querySelector("section")?.textContent).toContain(second.cwd);
  expect(button("Create & connect").disabled).toBe(true);
  await click(button("Create & connect"));
  expect(mocks.create).not.toHaveBeenCalled();
  await click(checkbox(second.title));
  expect(foreman()).toBeNull();
  expect(document.querySelector("section")?.textContent).toContain(source.cwd);
  expect(button("Create & connect").disabled).toBe(false);
  await click(button("Create & connect"));
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith([source.source_id]);
});

it("preserves selection order across targets and offline polls, then joins the explicit Group", async () => {
  vi.useFakeTimers();
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
  const select = vi.fn();
  await open(select);
  await click(checkbox(second.title));
  await click(checkbox(source.title));
  await click(target());
  expect(
    Array.from(document.querySelectorAll('[role="menuitemradio"]')).map((item) => item.textContent),
  ).toEqual([expect.stringContaining("+ New Group"), "Existing Group", "g-untitled"]);
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
    );
  });
  expect((document.activeElement as HTMLElement).dataset.value).toBe(existingGroup.group_id);
  await act(async () => {
    document.activeElement?.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  });
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  expect(document.activeElement).toBe(target());
  await click(target());
  await click(document.querySelector('[role="menuitemradio"][data-value="g-existing"]'));
  expect(button("Connect to Group").disabled).toBe(false);
  expect(document.querySelector('section[aria-label="New Group defaults"]')).toBeNull();
  expect(document.body.textContent).toContain("Selected sessions join this Group as Actors.");
  expect(document.body.textContent).not.toContain(enLayout.localSessions.groupHint);
  await chooseTarget("");
  expect(document.querySelector("section dd")?.textContent).toBe(second.title);
  expect(button("Create & connect").disabled).toBe(false);
  await chooseTarget(existingGroup.group_id);
  mocks.fetch.mockResolvedValueOnce({ ok: true, result: { sources: [source] } });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(checkbox(second.title).checked).toBe(true);
  expect(checkbox(second.title).disabled).toBe(false);
  expect(checkbox(source.title).checked).toBe(true);
  expect(button("Connect to Group").disabled).toBe(true);
  await click(button("Connect to Group"));
  expect(mocks.join).not.toHaveBeenCalled();
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(button("Connect to Group").disabled).toBe(false);
  await click(button("Connect to Group"));
  expect(mocks.join).toHaveBeenCalledExactlyOnceWith(existingGroup.group_id, [
    second.source_id,
    source.source_id,
  ]);
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.refresh).toHaveBeenCalledOnce();
  expect(mocks.refreshActors).toHaveBeenCalledExactlyOnceWith(existingGroup.group_id);
  expect(select).toHaveBeenCalledExactlyOnceWith(existingGroup.group_id);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  await act(async () => vi.advanceTimersByTimeAsync(9000));
  expect(mocks.fetch).toHaveBeenCalledTimes(3);
});

it("refreshes the real current Group and roster without re-enabling a paused member", async () => {
  const original = await vi.importActual<typeof import("../../services/api/actors")>(
    "../../services/api/actors",
  );
  mocks.members.mockImplementation(original.fetchActors);
  const paused: Actor = {
    id: "existing-disabled",
    role: "foreman",
    enabled: false,
    running: false,
  };
  const native: Actor = { id: "new-native", role: "peer", enabled: true, running: false };
  useGroupStore.setState({
    groupDoc: { ...existingGroup, title: "Stale title", scopes: undefined },
    actors: [paused],
    refreshGroups: useGroupStore.getInitialState().refreshGroups,
    refreshActors: useGroupStore.getInitialState().refreshActors,
  });
  const fetch = vi.fn(async (path: string) => {
    if (path === "/api/v1/groups") {
      return new Response(JSON.stringify({ ok: true, result: { groups: [existingGroup] } }));
    }
    if (
      path === `/api/v1/groups/${existingGroup.group_id}/actors` ||
      path ===
        `/api/v1/groups/${existingGroup.group_id}/actors?include_unread=true&include_internal=true`
    ) {
      return new Response(JSON.stringify({ ok: true, result: { actors: [paused, native] } }));
    }
    throw new Error(`Unexpected request: ${path}`);
  });
  vi.stubGlobal("fetch", fetch);
  const select = vi.fn((groupId: string) => useGroupStore.getState().setSelectedGroupId(groupId));
  await open(select);
  await chooseTarget(existingGroup.group_id);
  await click(checkbox(source.title));
  await click(button("Connect to Group"));

  const state = useGroupStore.getState();
  expect(select).toHaveBeenCalledExactlyOnceWith(existingGroup.group_id);
  expect(state.selectedGroupId).toBe(existingGroup.group_id);
  expect(state.groupDoc).toMatchObject(existingGroup);
  expect(state.groups).toEqual([existingGroup]);
  expect(state.actors).toEqual([paused, native]);
  expect(state.selectedGroupActorsHydrating).toBe(false);
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(fetch.mock.calls.map(([path]) => path)).toEqual([
    `/api/v1/groups/${existingGroup.group_id}/actors`,
    "/api/v1/groups",
    `/api/v1/groups/${existingGroup.group_id}/actors?include_unread=true&include_internal=true`,
  ]);
});

it("shows registered but unready sources without allowing their admission", async () => {
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: { sources: [{ ...source, connected: false }] },
  });
  await open();
  expect(row(source.title).textContent).toContain(enLayout.localSessions.unavailable);
  expect(checkbox(source.title).disabled).toBe(true);
  expect(document.body.textContent).not.toContain(enLayout.localSessions.empty);
  expect(button("Create & connect").disabled).toBe(true);
  // The hint names no cause; it leads to the troubleshooting steps.
  expect(document.body.textContent).toContain(enLayout.localSessions.unavailableHint);
  const [setupDetails, troubleshooting] = Array.from(document.querySelectorAll("details"));
  expect(setupDetails?.open).toBe(false);
  await click(button(enLayout.localSessions.showTroubleshooting));
  expect(setupDetails?.open).toBe(true);
  expect(troubleshooting?.open).toBe(true);
  // The reader can still close what was opened for them.
  await click(setupDetails!.querySelector("summary"));
  expect(setupDetails?.open).toBe(false);
});

it("marks existing members and resumes only a paused source on an explicit user action", async () => {
  const member: Actor = {
    id: "native-paused",
    title: "Original member",
    external_session_id: source.source_id,
    enabled: false,
  };
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [member] } });
  const select = vi.fn();
  await open(select);
  await click(checkbox(source.title));
  await chooseTarget(existingGroup.group_id);
  expect(checkbox(source.title).checked).toBe(false);
  expect(checkbox(source.title).disabled).toBe(true);
  expect(row(source.title).textContent).toContain(enLayout.localSessions.memberPaused);
  expect(mocks.resume).not.toHaveBeenCalled();
  expect(button("Connect to Group").disabled).toBe(true);
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [{ ...member, enabled: true }] } });
  await click(button(enActors.dotsResume));
  expect(mocks.resume).toHaveBeenCalledExactlyOnceWith(
    existingGroup.group_id,
    member.id,
    source.source_id,
  );
  expect(row(source.title).textContent).toContain(enLayout.localSessions.memberJoined);
  expect(document.body.textContent).toContain("CCCC access resumed for Original member.");
  expect(button(enActors.dotsResume)).toBeUndefined();
  expect(mocks.join).not.toHaveBeenCalled();
  expect(useGroupStore.getState().groupDoc?.state).toBe("paused");
  await click(button("Connect to Group"));
  expect(select).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.join).not.toHaveBeenCalled();
});

it("does not apply stale membership results from a previous target", async () => {
  let finish!: (value: unknown) => void;
  mocks.members.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await open();
  await chooseTarget(existingGroup.group_id);
  expect(checkbox(source.title).disabled).toBe(true);
  await chooseTarget("g-untitled");
  await act(async () =>
    finish({
      ok: true,
      result: { actors: [{ id: "old", enabled: false, external_session_id: source.source_id }] },
    }),
  );
  expect(checkbox(source.title).disabled).toBe(false);
  expect(document.body.textContent).not.toContain(enLayout.localSessions.memberPaused);
});

it("keeps admission disabled while membership cannot be confirmed and surfaces a failed resume", async () => {
  vi.useFakeTimers();
  mocks.members.mockResolvedValueOnce({ ok: false, error: { message: "Synthetic unavailable" } });
  await open();
  await chooseTarget(existingGroup.group_id);
  expect(document.body.textContent).toContain(enLayout.localSessions.membersFailed);
  expect(checkbox(source.title).disabled).toBe(true);
  expect(button("Connect to Group").disabled).toBe(true);
  mocks.members.mockResolvedValue({
    ok: true,
    result: { actors: [{ id: "paused", external_session_id: source.source_id, enabled: false }] },
  });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  mocks.resume.mockResolvedValueOnce({ ok: false, error: { message: "Member changed" } });
  await click(button(enActors.dotsResume));
  expect(mocks.error).toHaveBeenCalledWith("Member changed");
  expect(row(source.title).textContent).toContain(enLayout.localSessions.memberPaused);
  expect(mocks.join).not.toHaveBeenCalled();
});

it("does not silently create a new Group if the selected existing target disappears", async () => {
  await open();
  await click(checkbox(source.title));
  await chooseTarget(existingGroup.group_id);
  await act(async () => useGroupStore.setState({ groups: [{ group_id: "g-untitled" }] }));
  expect(target().dataset.value).toBe(existingGroup.group_id);
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("Choose another target.");
  expect(button("Connect to Group").disabled).toBe(true);
  await click(button("Connect to Group"));
  expect(mocks.join).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
  await chooseTarget("");
  expect(checkbox(source.title).checked).toBe(true);
  expect(button("Create & connect").disabled).toBe(false);
});

it.each(["missing", "disconnected"])(
  "keeps both selections and the multi-Group action during a %s poll, then submits both after recovery",
  async (unavailable) => {
    vi.useFakeTimers();
    mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
    await open();
    await click(checkbox(source.title));
    await click(checkbox(second.title));
    mocks.fetch.mockResolvedValueOnce({
      ok: true,
      result: {
        sources: unavailable === "missing" ? [source] : [source, { ...second, connected: false }],
      },
    });
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(checkbox(source.title).checked).toBe(true);
    expect(checkbox(second.title).checked).toBe(true);
    expect(checkbox(second.title).disabled).toBe(false);
    expect(row(second.title).textContent).toContain("Unavailable to receive");
    expect(document.body.textContent).toContain("Selected sessions: 2 / 16");
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      "Deselect them or wait for reconnection.",
    );
    expect(button("Create & connect").disabled).toBe(true);
    await click(button("Create & connect"));
    expect(mocks.create).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(3000));
    expect(checkbox(second.title).checked).toBe(true);
    expect(document.querySelector('[role="alert"]')).toBeNull();
    expect(button("Create & connect").disabled).toBe(false);
    await click(button("Create & connect"));
    expect(mocks.create).toHaveBeenCalledExactlyOnceWith([source.source_id, second.source_id]);
  },
);

it("retains a single unavailable selection when the entire connected directory becomes empty", async () => {
  vi.useFakeTimers();
  await open();
  await click(checkbox(source.title));
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [] } });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(checkbox(source.title).checked).toBe(true);
  expect(checkbox(source.title).disabled).toBe(false);
  expect(document.body.textContent).toContain("Selected sessions: 1 / 16");
  expect(button("Create & connect").disabled).toBe(true);
  expect(document.body.textContent).not.toContain(enLayout.localSessions.empty);
  await click(checkbox(source.title));
  expect(document.body.textContent).toContain("Selected sessions: 0 / 16");
  expect(document.body.textContent).toContain(enLayout.localSessions.empty);
});

it("allows explicit removal of an unavailable selection without silently changing the remaining selection", async () => {
  vi.useFakeTimers();
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
  await open();
  await click(checkbox(second.title));
  await click(checkbox(source.title));
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source] } });
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(button("Create & connect").disabled).toBe(true);
  await click(checkbox(second.title));
  expect(document.body.textContent).toContain("Selected sessions: 1 / 16");
  expect(button("Create & connect").disabled).toBe(false);
  await click(button("Create & connect"));
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith([source.source_id]);
});

it.each(["new", "existing"])(
  "enforces the 16-session limit for the %s target without disabling removal",
  async (kind) => {
    const joining = kind === "existing";
    const action = joining ? "Connect to Group" : "Create & connect";
    const sources = Array.from({ length: 17 }, (_, index) => ({
      ...source,
      source_id: `codex:session-${index}`,
      title: `Session ${String(index).padStart(2, "0")}`,
    }));
    mocks.fetch.mockResolvedValue({ ok: true, result: { sources } });
    await open();
    if (joining) await chooseTarget(existingGroup.group_id);
    for (const item of sources.slice(0, 16)) await click(checkbox(item.title));
    expect(document.body.textContent).toContain("Selected sessions: 16 / 16");
    expect(checkbox(sources[16].title).disabled).toBe(true);
    expect(checkbox(sources[0].title).disabled).toBe(false);
    expect(button(action).disabled).toBe(false);
    await click(checkbox(sources[0].title));
    expect(checkbox(sources[16].title).disabled).toBe(false);
    await click(checkbox(sources[16].title));
    await click(button(action));
    const sourceIds = sources.slice(1).map((item) => item.source_id);
    if (joining)
      expect(mocks.join).toHaveBeenCalledExactlyOnceWith(existingGroup.group_id, sourceIds);
    else expect(mocks.create).toHaveBeenCalledExactlyOnceWith(sourceIds);
  },
);

it("does not present a failed directory load as an empty session directory and recovers on the existing poll", async () => {
  vi.useFakeTimers();
  mocks.fetch.mockResolvedValueOnce({
    ok: false,
    error: { message: "Synthetic directory failure" },
  });
  await open();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not refresh native sessions.",
  );
  expect(document.body.textContent).toContain("Synthetic directory failure");
  expect(document.body.textContent).not.toContain("No native sessions are available");
  expect(button("Create & connect").disabled).toBe(true);
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(document.querySelector('[role="alert"]')).toBeNull();
  await click(checkbox(source.title));
  expect(button("Create & connect").disabled).toBe(false);
});

it("requires a selected session when connecting to an existing Group instead of navigating", async () => {
  const select = vi.fn();
  await open(select);
  await chooseTarget(existingGroup.group_id);
  expect(button("Connect to Group").disabled).toBe(true);
  await click(button("Connect to Group"));
  expect(select).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.join).not.toHaveBeenCalled();
  await click(checkbox(source.title));
  expect(button("Connect to Group").disabled).toBe(false);
});

it("distinguishes Codex creation origins without merging sessions in the same directory", async () => {
  const ide = {
    ...source,
    source_id: "codex:ide",
    native_session_id: "ide",
    title: "Codex · IDE session",
    origin: "vscode",
  };
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: { sources: [{ ...source, origin: "cli" }, ide] },
  });
  await open();
  expect(row(source.title).textContent).toContain("CLI");
  expect(row(ide.title).textContent).toContain("VS Code");
  expect(row(ide.title).querySelector('[title="Session created from VS Code"]')).not.toBeNull();
  await click(checkbox(source.title));
  await click(checkbox(ide.title));
  await click(button("Create & connect"));
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith([source.source_id, ide.source_id]);
});

it.each([undefined, "future-provider-origin"])(
  "keeps a session selectable when its optional origin is %s",
  async (origin) => {
    mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [{ ...source, origin }] } });
    await open();
    expect(row(source.title).querySelector('[title^="Session created from"]')).toBeNull();
    expect(checkbox(source.title).disabled).toBe(false);
    await click(checkbox(source.title));
    expect(button("Create & connect").disabled).toBe(false);
  },
);

it.each([
  { count: 1, joining: false, action: "Create & connect", pending: "Creating & connecting…" },
  { count: 2, joining: false, action: "Create & connect", pending: "Creating & connecting…" },
  { count: 2, joining: true, action: "Connect to Group", pending: "Connecting…" },
])(
  "announces $pending, prevents duplicates, and refreshes after closing without navigating away",
  async ({ count, joining, action, pending: pendingLabel }) => {
    vi.useFakeTimers();
    let resolve!: (value: unknown) => void;
    mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
    const mutate = joining ? mocks.join : mocks.create;
    mutate.mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );
    const select = vi.fn();
    await open(select);
    if (joining) await chooseTarget(existingGroup.group_id);
    await click(checkbox(source.title));
    if (count === 2) await click(checkbox(second.title));
    const submit = button(action);
    await act(async () => {
      submit.click();
      submit.click();
    });
    const pending = button(pendingLabel);
    expect(pending.disabled).toBe(true);
    expect(pending.getAttribute("aria-busy")).toBe("true");
    expect(document.body.textContent).toContain(`Selected sessions: ${count} / 16`);
    expect(checkbox(source.title).disabled).toBe(true);
    expect(target().disabled).toBe(true);
    if (foreman()) expect(foreman()?.disabled).toBe(true);
    await click(pending);
    expect(mutate).toHaveBeenCalledOnce();
    await click(document.body.querySelector('button[aria-label="Close"]'));
    const otherGroup: GroupDoc = { group_id: "g-other", title: "Other Group" };
    useGroupStore.setState({ selectedGroupId: otherGroup.group_id, groupDoc: otherGroup });
    await act(async () => {
      resolve({
        ok: true,
        result: joining
          ? { group_id: existingGroup.group_id, group: existingGroup, reused: false }
          : { group_id: "g-created" },
      });
    });
    expect(mocks.refresh).toHaveBeenCalledOnce();
    if (joining)
      expect(mocks.refreshActors).toHaveBeenCalledExactlyOnceWith(existingGroup.group_id);
    else expect(mocks.refreshActors).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
    expect(useGroupStore.getState().groupDoc).toBe(otherGroup);
    await act(async () => vi.advanceTimersByTimeAsync(9000));
    expect(mocks.fetch).toHaveBeenCalledOnce();
  },
);

it.each(["new", "existing"])(
  "reports a failed %s target mutation without losing selection, navigating or retrying automatically",
  async (kind) => {
    vi.useFakeTimers();
    const joining = kind === "existing";
    const mutate = joining ? mocks.join : mocks.create;
    const action = joining ? "Connect to Group" : "Create & connect";
    mutate.mockResolvedValueOnce({ ok: false, error: { message: "Synthetic mutation failure" } });
    mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
    const select = vi.fn();
    await open(select);
    if (joining) await chooseTarget(existingGroup.group_id);
    await click(checkbox(second.title));
    await click(checkbox(source.title));
    await click(button(action));
    expect(mocks.error).toHaveBeenCalledExactlyOnceWith("Synthetic mutation failure");
    expect(mutate).toHaveBeenCalledOnce();
    expect(mocks.refresh).not.toHaveBeenCalled();
    expect(mocks.refreshActors).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
    expect(checkbox(source.title).checked).toBe(true);
    expect(checkbox(second.title).checked).toBe(true);
    expect(button(action).disabled).toBe(false);
    expect(button(action).getAttribute("aria-busy")).toBe("false");
    await act(async () => vi.advanceTimersByTimeAsync(9000));
    expect(mutate).toHaveBeenCalledOnce();
    expect(select).not.toHaveBeenCalled();
    expect(target().dataset.value).toBe(joining ? existingGroup.group_id : "");
    await click(button(action));
    if (joining) {
      expect(mocks.join).toHaveBeenLastCalledWith(existingGroup.group_id, [
        second.source_id,
        source.source_id,
      ]);
      expect(mocks.create).not.toHaveBeenCalled();
    } else expect(mocks.create).toHaveBeenLastCalledWith([second.source_id, source.source_id]);
    expect(select).toHaveBeenCalledExactlyOnceWith(joining ? existingGroup.group_id : "g-created");
  },
);

it("offers the terminal /setup path, the enable commands and a collapsed troubleshooting section", async () => {
  await open();
  // Sessions are already connected, so setup starts collapsed.
  expect(document.querySelector("details")?.open).toBe(false);
  expect(document.body.textContent).not.toContain(enLayout.localSessions.unavailableHint);
  await click(document.querySelector("summary"));
  const [setupDetails, troubleshooting] = Array.from(document.querySelectorAll("details"));
  expect(setupDetails?.open).toBe(true);
  const setup = setupDetails!.textContent!;
  for (const text of [
    "message each other and join Groups",
    "Experimental",
    "running cccc in a terminal, type /setup there",
    "same OS user",
    "CCCC_HOME",
    "cccc sessions enable --runtime codex",
    "cccc sessions enable --runtime claude",
    "cccc sessions enable --runtime pi",
    "cccc sessions enable --runtime opencode",
    "approve the CCCC hooks when asked (/hooks)",
    "/reload-plugins",
    "Pi: run /reload",
    "OpenCode V2: reopen the original session",
  ]) {
    expect(setup).toContain(text);
  }
  expect(setup).not.toMatch(/\d+\.\d+\.\d+/);
  const example = setupDetails!.querySelector("blockquote")?.textContent || "";
  expect(example).toContain("/path/to/project");
  expect(example).toContain("reply through CCCC using Send");
  expect(example).toContain("If several sessions match, ask me to choose");
  expect(setup).toContain("you do not need to create one first");

  // Troubleshooting stays available without crowding the main steps.
  expect(troubleshooting?.open).toBe(false);
  const help = troubleshooting!.textContent!;
  for (const text of [
    "Troubleshooting",
    "cccc sessions list",
    "native prompt",
    "shared app-server",
    "daemon_auto_start = true",
    "[features]",
    "config.toml",
    "Monitor",
  ]) {
    expect(help).toContain(text);
  }
  expect(setupDetails!.querySelector(":scope > a")?.getAttribute("href")).toBe(
    "https://chesterra.github.io/cccc/guide/local-sessions",
  );
});

it.each([
  [
    "en",
    "Connect existing sessions",
    "Connect existing sessions",
    "Create & connect",
    "Create & connect",
    "Selected sessions: 2 / 16",
    "Connect to Group",
  ],
  [
    "zh",
    "接入已有会话",
    "接入已有会话",
    "创建并接入",
    "创建并接入",
    "已选会话：2 / 16",
    "接入工作组",
  ],
  [
    "ja",
    "既存セッションを接続",
    "既存セッションを接続",
    "作成して接続",
    "作成して接続",
    "選択したセッション：2 / 16",
    "グループに接続",
  ],
])(
  "keeps native-session names, actions and counts aligned in %s",
  async (language, entry, title, single, multiple, count, join) => {
    await i18n.changeLanguage(language);
    mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source, second] } });
    await mount(vi.fn(), true);
    const entryButton = host.querySelector<HTMLButtonElement>(`button[aria-label="${entry}"]`)!;
    expect(entryButton.title).toBe(entry);
    await click(entryButton);
    expect(document.getElementById("local-sessions-title")?.textContent).toBe(title);
    await click(checkbox(source.title));
    expect(button(single).disabled).toBe(false);
    await click(checkbox(second.title));
    expect(button(multiple).disabled).toBe(false);
    expect(foreman()?.getAttribute("aria-label")).toBe(i18n.t("actors:groupRoleForeman"));
    expect(document.body.textContent).toContain(count);
    await click(document.querySelector("summary"));
    const example = document.querySelector("details blockquote")?.textContent || "";
    for (const text of ["CCCC", "Codex", "/path/to/project", "Send + Reply"]) {
      expect(example).toContain(text);
    }
    await chooseTarget(existingGroup.group_id);
    expect(foreman()).toBeNull();
    expect(button(join).disabled).toBe(false);
    expect(target().textContent).toContain(existingGroup.title);
  },
);
