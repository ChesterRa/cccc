// @vitest-environment happy-dom
import { createInstance } from "i18next";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, LocalSessionSource } from "../../types";
import { useGroupStore, useUIStore } from "../../stores";
import { ModalFrame } from "../../components/modals/ModalFrame";
import { CreateGroupModal } from "../../components/modals/CreateGroupModal";
import { setAuthToken } from "../../services/api/base";
import enLayout from "../../i18n/locales/en/layout.json";
import enModals from "../../i18n/locales/en/modals.json";
import zhModals from "../../i18n/locales/zh/modals.json";
import jaModals from "../../i18n/locales/ja/modals.json";
import { verifyPairingTicketFlow } from "./LocalSessionsDevice.actions";
import { sessionPickerActions } from "./LocalSessionsPicker.actions";
import { LocalSessionsPicker } from "./LocalSessionsPicker";
import { source, second, existingGroup, resources } from "./LocalSessionsButton.fixtures";
import { pairedComputerStates } from "./sessionNodeStatus.fixtures";

const mocks = vi.hoisted(() => ({
  local: vi.fn(),
  deviceSources: vi.fn(),
  deviceStatus: vi.fn(),
  create: vi.fn(),
  join: vi.fn(),
  members: vi.fn(),
  resume: vi.fn(),
  refresh: vi.fn(),
  refreshActors: vi.fn(),
  error: vi.fn(),
}));
vi.mock("../../services/api/localSessions", () => ({
  fetchLocalSessions: mocks.local,
  createLocalConversation: mocks.create,
  joinLocalSessions: mocks.join,
  resumeLocalSessionAccess: mocks.resume,
}));
vi.mock("../../services/api/sessionNodes", async (original) => ({
  ...(await original<typeof import("../../services/api/sessionNodes")>()),
  fetchDeviceSessionSources: mocks.deviceSources,
  fetchSessionNodes: mocks.deviceStatus,
}));
vi.mock("../../services/api/actors", async (original) => ({
  ...(await original<typeof import("../../services/api/actors")>()),
  fetchActors: mocks.members,
}));
vi.mock("../../components/ActorAvatar", () => ({ ActorAvatar: () => <span /> }));

const i18n = createInstance();
const t = (key: string, options?: Record<string, unknown>) => i18n.t(key, options);
const { click, button, row, checkbox, foreman, chooseForeman } = sessionPickerActions(t);
const joinLabel = (count = 1) => t("localSessions.attachSessions", { count });
const main = (count = 1) => button(joinLabel(count));
const create = () => button(t("modals:createGroup.createGroup"));
const waitPoll = () => act(async () => vi.advanceTimersByTimeAsync(3000));
const select = vi.fn();
const close = vi.fn();
const group = existingGroup.group_id;
const currentComputer = {
  node_id: second.node_id,
  name: second.node_name,
  connected: true,
  enabled: true,
};
const ready = (devices = [currentComputer]) => ({
  ok: true,
  result: { supported: true, devices, supported_runtimes: ["codex", "claude"] },
});
const okSources = (sources: LocalSessionSource[]) => ({ ok: true, result: { sources } });
const paused: Actor = {
  id: "native-member",
  title: "Original member",
  external_session_id: source.source_id,
  enabled: false,
};
let host: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.resetAllMocks();
  vi.useFakeTimers();
  setAuthToken("");
  const otherGroup = { group_id: "g-other", title: "Other Group" };
  useGroupStore.setState({
    ...useGroupStore.getInitialState(),
    groups: [existingGroup, otherGroup],
    selectedGroupId: group,
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
        en: { ...resources.en, modals: enModals },
        zh: { ...resources.zh, modals: zhModals },
        ja: { ...resources.ja, modals: jaModals },
      },
      interpolation: { escapeValue: false },
    });
  mocks.local.mockResolvedValue(okSources([source]));
  mocks.deviceSources.mockResolvedValue(okSources([second]));
  mocks.deviceStatus.mockResolvedValue(ready());
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [] } });
  mocks.resume.mockResolvedValue({ ok: true });
  mocks.create.mockResolvedValue({ ok: true, result: { group_id: "g-created", reused: false } });
  mocks.join.mockResolvedValue({
    ok: true,
    result: { group_id: group, group: existingGroup, reused: false },
  });
  mocks.refresh.mockResolvedValue(undefined);
  mocks.refreshActors.mockResolvedValue(undefined);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  close.mockImplementation(() => root.render(null));
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useGroupStore.setState(useGroupStore.getInitialState(), true);
  useUIStore.setState(useUIStore.getInitialState(), true);
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function mount(groupId: string | null = group, active = true) {
  await act(async () =>
    root.render(
      <I18nextProvider i18n={i18n}>
        <ModalFrame
          isDark={false}
          titleId="fixture-title"
          title="Fixture"
          closeAriaLabel={t("common:close")}
          panelClassName="h-full"
          onClose={close}
        >
          <LocalSessionsPicker
            key={groupId || "new"}
            groupId={groupId}
            active={active}
            isDark={false}
            onClose={close}
            onSelectGroup={select}
            onBusyChange={() => {}}
          />
        </ModalFrame>
      </I18nextProvider>,
    ),
  );
}

function defer() {
  let resolve!: (value: unknown) => void;
  const response = new Promise((done) => {
    resolve = done;
  });
  return { resolve, response };
}

it("reads only the fixed Group roster and polls only while the existing-session list is active", async () => {
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [paused] } });
  await mount(group, false);
  await waitPoll();
  expect(mocks.local).not.toHaveBeenCalled();
  expect(mocks.deviceSources).not.toHaveBeenCalled();
  expect(mocks.members).not.toHaveBeenCalled();
  await mount();
  expect(mocks.members).toHaveBeenCalledExactlyOnceWith(group, false, { noCache: true });
  expect(row(source.title).textContent).toContain(enLayout.localSessions.memberPaused);
  expect(button(t("localSessions.resumeGroupAccess"))).toBeDefined();
  await waitPoll();
  expect(mocks.members).toHaveBeenCalledTimes(2);
  await mount(group, false);
  await waitPoll();
  expect(mocks.members).toHaveBeenCalledTimes(2);
  expect(mocks.local).toHaveBeenCalledTimes(2);
  expect(mocks.deviceSources).toHaveBeenCalledTimes(2);
});

it("separates initial loading, empty help, and discovery with no write", async () => {
  const pending = defer();
  mocks.local.mockReturnValueOnce(pending.response);
  await mount(null);
  expect(host.querySelector('[aria-busy="true"]')?.textContent).toContain(t("common:loading"));
  expect(host.textContent).not.toContain(enLayout.localSessions.empty);
  expect(create().disabled).toBe(true);
  await act(async () => pending.resolve(okSources([])));
  expect(host.textContent).toContain(enLayout.localSessions.empty);
  const [setup, troubleshooting] = Array.from(host.querySelectorAll("details"));
  expect(setup?.open).toBe(true);
  expect(troubleshooting?.open).toBe(false);
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.join).not.toHaveBeenCalled();
});

it.each([true, false])(
  "only exposes existing-session creation with admin permission (%s) and requires no local project path",
  async (isWebAdmin) => {
    const createBlank = vi.fn();
    useGroupStore.setState({ groups: [], selectedGroupId: "", groupDoc: null });
    useUIStore.setState({ isWebAdmin, webReadOnly: false });
    await act(async () =>
      root.render(
        <I18nextProvider i18n={i18n}>
          <CreateGroupModal
            isOpen
            isDark={false}
            busy=""
            dirSuggestions={[]}
            dirItems={[]}
            currentDir=""
            parentDir={null}
            showDirBrowser={false}
            createGroupPath=""
            setCreateGroupPath={vi.fn()}
            createGroupName=""
            setCreateGroupName={vi.fn()}
            creatingDirectory={false}
            onFetchDirContents={vi.fn()}
            onCreateDirectory={async () => false}
            onCreateGroup={createBlank}
            onClose={close}
            onCancelAndReset={close}
            onSelectGroup={select}
          />
        </I18nextProvider>,
      ),
    );
    const fromExisting = button(t("modals:createGroup.fromExistingSessions"));
    expect(mocks.local).not.toHaveBeenCalled();
    expect(mocks.deviceSources).not.toHaveBeenCalled();
    expect(mocks.deviceStatus).not.toHaveBeenCalled();
    await click(fromExisting);
    if (!isWebAdmin) {
      expect(fromExisting.getAttribute("aria-disabled")).toBe("true");
      expect(mocks.local).not.toHaveBeenCalled();
      expect(mocks.deviceSources).not.toHaveBeenCalled();
      return;
    }
    expect(mocks.members).not.toHaveBeenCalled();
    await click(checkbox(second.title));
    expect(create().disabled).toBe(false);
    await click(create());
    expect(createBlank).not.toHaveBeenCalled();
    expect(mocks.join).not.toHaveBeenCalled();
    expect(mocks.create).toHaveBeenCalledExactlyOnceWith([second.source_id], "");
    expect(select).toHaveBeenCalledWith("g-created");
  },
);

it("keeps a mixed choice and restores the focus when leaving inline pairing", async () => {
  await mount();
  await click(checkbox(second.title));
  await click(checkbox(source.title));
  const trigger = button(t("localSessions.addDevice"));
  await click(trigger);
  const counts = [
    mocks.local.mock.calls.length,
    mocks.deviceSources.mock.calls.length,
    mocks.members.mock.calls.length,
  ];
  await waitPoll();
  expect([
    mocks.local.mock.calls.length,
    mocks.deviceSources.mock.calls.length,
    mocks.members.mock.calls.length,
  ]).toEqual(counts);
  expect(host.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  expect(host.textContent).toContain(t("localSessions.pairSelectionKept", { count: 2 }));
  await click(button(t("sessionNodes.backToSessionList")));
  await act(async () => vi.advanceTimersByTimeAsync(32));
  expect(checkbox(source.title).checked).toBe(true);
  expect(checkbox(second.title).checked).toBe(true);
  expect(document.activeElement).toBe(trigger);
  expect([
    mocks.local.mock.calls.length,
    mocks.deviceSources.mock.calls.length,
    mocks.members.mock.calls.length,
  ]).toEqual(counts.map((n) => n + 1));
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.join).not.toHaveBeenCalled();
  await click(main(2));
  expect(mocks.join).toHaveBeenCalledExactlyOnceWith(group, [second.source_id, source.source_id]);
  expect(mocks.refreshActors).toHaveBeenCalledExactlyOnceWith(group);
});

it("preserves the full pairing ticket and clipboard flow at the inline entry", async () => {
  await verifyPairingTicketFlow(() => mount(null));
});

it("previews only local cwd, uses an explicit title, and submits the chosen Foreman first", async () => {
  const third = {
    ...source,
    source_id: "codex:third",
    title: "Another local",
    cwd: "/another/dir",
  };
  mocks.local.mockResolvedValue(okSources([source, third]));
  await mount(null);
  expect(host.textContent).toContain(t("localSessions.chooseDirectorySource"));
  await click(checkbox(second.title));
  expect(host.textContent).toContain(t("modals:createGroup.noLocalProjectDirectory"));
  expect(foreman()).toBeNull();
  await click(checkbox(source.title));
  await click(checkbox(third.title));
  expect(host.textContent).toContain(t("localSessions.foremanDirectoryHint"));
  await chooseForeman(third.source_id);
  expect(foreman()?.dataset.value).toBe(third.source_id);
  const input = host.querySelector<HTMLInputElement>('input:not([type="checkbox"])')!;
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
    setter?.call(input, "Named Group");
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await click(create());
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith(
    [third.source_id, second.source_id, source.source_id],
    "Named Group",
  );
  expect(mocks.join).not.toHaveBeenCalled();
  expect(mocks.refreshActors).not.toHaveBeenCalled();
  expect(select).toHaveBeenCalledWith("g-created");
});

it("keeps the only unavailable choice cancellable after the entire local directory becomes empty", async () => {
  await mount(null);
  await click(checkbox(source.title));
  mocks.local.mockResolvedValue(okSources([]));
  await waitPoll();
  expect(checkbox(source.title).checked).toBe(true);
  expect(checkbox(source.title).disabled).toBe(false);
  expect(create().disabled).toBe(true);
  await click(checkbox(source.title));
  expect(host.textContent).toContain(enLayout.localSessions.empty);
  expect(create().disabled).toBe(true);
  expect(mocks.create).not.toHaveBeenCalled();
});

it.each(["missing", "offline"])(
  "keeps a %s remote choice cancellable until it returns",
  async (state) => {
    await mount(null);
    await click(checkbox(second.title));
    await click(checkbox(source.title));
    if (state === "offline")
      mocks.deviceStatus.mockResolvedValue(ready([{ ...currentComputer, connected: false }]));
    else mocks.deviceSources.mockResolvedValue(okSources([]));
    await act(async () => vi.advanceTimersByTimeAsync(5000));
    const unavailable = host.querySelector(
      `section[aria-label="${t("localSessions.unavailableSelected")}"]`,
    );
    expect(unavailable?.textContent).toContain(second.title);
    expect(checkbox(second.title).checked).toBe(true);
    expect(checkbox(second.title).disabled).toBe(false);
    expect(create().disabled).toBe(true);
    expect(mocks.create).not.toHaveBeenCalled();
    mocks.deviceStatus.mockResolvedValue(ready());
    mocks.deviceSources.mockResolvedValue(okSources([second]));
    await act(async () => vi.advanceTimersByTimeAsync(5000));
    expect(checkbox(second.title).checked).toBe(true);
    expect(create().disabled).toBe(false);
    await click(create());
    expect(mocks.create).toHaveBeenCalledExactlyOnceWith([second.source_id, source.source_id], "");
  },
);

it("allows dismissing a revoked remote selection and submits only the remaining local source", async () => {
  await mount(null);
  await click(checkbox(second.title));
  await click(checkbox(source.title));
  mocks.deviceStatus.mockResolvedValue(
    ready([{ ...currentComputer, enabled: false, connected: false }]),
  );
  mocks.deviceSources.mockResolvedValue(okSources([]));
  await act(async () => vi.advanceTimersByTimeAsync(5000));
  expect(create().disabled).toBe(true);
  await click(checkbox(second.title));
  expect(create().disabled).toBe(false);
  await click(create());
  expect(mocks.create).toHaveBeenCalledExactlyOnceWith([source.source_id], "");
});

it.each([null, group])(
  "enforces the 16-session limit for target %s without blocking deselection",
  async (target) => {
    const sources = Array.from({ length: 17 }, (_, index) => ({
      ...source,
      source_id: `codex:limit-${index}`,
      title: `Session ${index + 1}`,
    }));
    mocks.local.mockResolvedValue(okSources(sources));
    await mount(target);
    for (const item of sources.slice(0, 16)) await click(checkbox(item.title));
    expect(checkbox(sources[16].title).disabled).toBe(true);
    expect(checkbox(sources[0].title).disabled).toBe(false);
    await click(checkbox(sources[0].title));
    await click(checkbox(sources[16].title));
    await click(target ? main(16) : create());
    const ids = sources.slice(1).map((item) => item.source_id);
    if (target) expect(mocks.join).toHaveBeenCalledExactlyOnceWith(target, ids);
    else expect(mocks.create).toHaveBeenCalledExactlyOnceWith(ids, "");
  },
);

it("blocks known existing members and resumes only a paused member on an explicit action", async () => {
  const joined = {
    ...paused,
    id: "remote-member",
    external_session_id: second.source_id,
    enabled: true,
  };
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [paused, joined] } });
  await mount();
  for (const item of [source, second]) {
    expect(checkbox(item.title).checked).toBe(false);
    expect(checkbox(item.title).disabled).toBe(true);
  }
  expect(mocks.resume).not.toHaveBeenCalled();
  mocks.members.mockResolvedValue({
    ok: true,
    result: { actors: [{ ...paused, enabled: true }, joined] },
  });
  await click(button(t("localSessions.resumeGroupAccess")));
  expect(mocks.resume).toHaveBeenCalledExactlyOnceWith(group, paused.id, source.source_id);
  expect(row(source.title).textContent).toContain(enLayout.localSessions.memberJoined);
  expect(button(t("localSessions.resumeGroupAccess"))).toBeUndefined();
  expect(useGroupStore.getState().groupDoc?.state).toBe("paused");
  expect(mocks.join).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
  expect(select).not.toHaveBeenCalled();
});

it("shows a membership error while blocking writes and preserves pause when resume fails", async () => {
  mocks.members.mockResolvedValue({ ok: false, error: { message: "Roster failed" } });
  await mount();
  await click(checkbox(second.title));
  expect(main().disabled).toBe(true);
  expect(host.textContent).toContain(enLayout.localSessions.membersFailed);
  mocks.members.mockResolvedValue({ ok: true, result: { actors: [paused] } });
  await waitPoll();
  mocks.resume.mockResolvedValue({ ok: false, error: { message: "Member changed" } });
  await click(button(t("localSessions.resumeGroupAccess")));
  expect(mocks.error).toHaveBeenCalledWith("Member changed");
  expect(row(source.title).textContent).toContain(enLayout.localSessions.memberPaused);
  expect(mocks.join).not.toHaveBeenCalled();
});

it("ignores a previous Group's delayed roster and closes when the active target is deleted", async () => {
  const pending = defer();
  mocks.members.mockReturnValueOnce(pending.response);
  await mount();
  await click(checkbox(source.title));
  expect(main().disabled).toBe(true);
  await mount("g-other");
  await act(async () => pending.resolve({ ok: true, result: { actors: [paused] } }));
  expect(checkbox(source.title).disabled).toBe(false);
  expect(host.textContent).not.toContain(enLayout.localSessions.memberPaused);
  await click(checkbox(source.title));
  await act(async () => useGroupStore.setState({ groups: [existingGroup] }));
  expect(close).toHaveBeenCalledOnce();
  expect(useUIStore.getState().notice?.message).toBe(t("localSessions.targetUnavailable"));
  expect(mocks.join).not.toHaveBeenCalled();
  expect(mocks.create).not.toHaveBeenCalled();
});

it("isolates a local read failure from remote-only admission and recovers by polling", async () => {
  mocks.local.mockResolvedValue({ ok: false, error: { message: "Local read failed" } });
  await mount();
  expect(host.textContent).toContain("Local read failed");
  expect(host.textContent).not.toContain(enLayout.localSessions.empty);
  await click(checkbox(second.title));
  expect(main().disabled).toBe(false);
  await click(main());
  expect(mocks.join).toHaveBeenCalledExactlyOnceWith(group, [second.source_id]);
  await mount();
  mocks.local.mockResolvedValue(okSources([]));
  await waitPoll();
  expect(host.textContent).not.toContain("Local read failed");
  expect(host.textContent).toContain(enLayout.localSessions.empty);
  expect(host.querySelector("details")?.open).toBe(true);
});

it.each([200, 404, 503])(
  "keeps local admission available with device-source HTTP %d",
  async (status) => {
    mocks.deviceStatus.mockResolvedValue(ready([]));
    mocks.deviceSources.mockResolvedValue(
      status === 200
        ? okSources([])
        : { ok: false, error: { httpStatus: status, message: "Device read failed" } },
    );
    await mount();
    expect(host.textContent?.includes("Device read failed")).toBe(status === 503);
    expect(host.textContent?.includes(t("sessionNodes.unpaired"))).toBe(status !== 503);
    await click(checkbox(source.title));
    expect(main().disabled).toBe(false);
    await click(main());
    expect(mocks.join).toHaveBeenCalledExactlyOnceWith(group, [source.source_id]);
  },
);

it.each(["unpaired", "offline", "online"] as const)(
  "distinguishes %s with the same empty device source response",
  async (state) => {
    mocks.deviceSources.mockResolvedValue(okSources([]));
    mocks.deviceStatus.mockResolvedValue(ready(pairedComputerStates[state]));
    await mount();
    expect(host.textContent?.includes(t("sessionNodes.unpaired"))).toBe(state === "unpaired");
    expect(host.textContent?.includes(t("sessionNodes.offline"))).toBe(state === "offline");
    expect(host.textContent?.includes(t("sessionNodes.onlineWithoutSessions"))).toBe(
      state === "online",
    );
  },
);

it("lets the local directory settle and poll independently before the device directory resolves", async () => {
  const pending = defer();
  mocks.deviceSources.mockReturnValue(pending.response);
  await mount();
  await click(checkbox(source.title));
  expect(main().disabled).toBe(false);
  expect(host.textContent).not.toContain(second.title);
  await waitPoll();
  expect(mocks.local).toHaveBeenCalledTimes(2);
  await act(async () => pending.resolve(okSources([second])));
  expect(checkbox(source.title).checked).toBe(true);
  expect(checkbox(second.title).disabled).toBe(false);
  await click(main());
  expect(mocks.join).toHaveBeenCalledExactlyOnceWith(group, [source.source_id]);
});

it("keeps same-name and local-looking computer sections distinct", async () => {
  const same = { ...currentComputer, node_id: "studio-node", name: currentComputer.name };
  const spoof = {
    ...currentComputer,
    node_id: "different-node",
    name: t("localSessions.thisDevice"),
  };
  mocks.deviceStatus.mockResolvedValue(ready([currentComputer, same, spoof]));
  await mount();
  const sections = Array.from(host.querySelectorAll("section"), (section) =>
    section.getAttribute("aria-label"),
  );
  expect(sections).toEqual(
    expect.arrayContaining([
      t("localSessions.thisDevice"),
      `${currentComputer.name} · ${currentComputer.node_id}`,
      `${same.name} · ${same.node_id.slice(0, 8)}`,
    ]),
  );
  expect(sections.filter((title) => title === spoof.name)).toHaveLength(2);
});

it("retains an action for an unavailable registered source and leaves the empty observation read-only", async () => {
  mocks.local.mockResolvedValue({
    ok: true,
    result: {
      sources: [{ ...source, connected: false }],
      capabilities: { source_control: true, source_diagnostics: true },
      source_states: {
        [source.source_id]: {
          diagnostic: { state: "thread_not_loaded" },
          control: { supported: true, access_blocked: false, pause_complete: true },
        },
      },
    },
  });
  await mount();
  expect(checkbox(source.title).disabled).toBe(true);
  expect(host.textContent).toContain(enLayout.localSessions.unavailable);
  expect(button(t("sourceControls.recheck"))).toBeDefined();
  expect(button(t("sourceControls.remove"))).toBeUndefined();
  expect(button(t("localSessions.attachSelectedSessions")).disabled).toBe(true);
  expect(mocks.create).not.toHaveBeenCalled();
  expect(mocks.join).not.toHaveBeenCalled();
  expect(mocks.resume).not.toHaveBeenCalled();
});

it("reports a recheck result without joining the source", async () => {
  const state = {
    diagnostic: { state: "thread_not_loaded" },
    control: { supported: true, access_blocked: false, pause_complete: true },
  };
  mocks.local.mockResolvedValue({
    ok: true,
    result: {
      sources: [{ ...source, connected: false }],
      capabilities: { source_control: true, source_diagnostics: true },
      source_states: { [source.source_id]: state },
    },
  });
  const pending = defer();
  const request = vi.fn((_url: string) => pending.response);
  vi.stubGlobal("fetch", request);
  await mount();
  await click(checkbox(second.title));
  await click(button(t("sourceControls.recheck")));
  expect(main().disabled).toBe(true);
  expect(main().getAttribute("aria-busy")).toBeNull();
  await act(async () =>
    pending.resolve(
      Response.json({
        ok: true,
        result: {
          source_id: source.source_id,
          diagnostic: state.diagnostic,
          check_status: "cached",
          retry_after_ms: 5000,
        },
      }),
    ),
  );
  expect(request).toHaveBeenCalledOnce();
  expect(request.mock.calls[0]?.[0]).toBe("/api/v1/local_sessions/recheck");
  expect(button(t("sourceControls.checkLater")).disabled).toBe(true);
  expect(host.textContent).toContain(t("sourceControls.checkCached"));
  expect(main().disabled).toBe(false);
  expect(mocks.join).not.toHaveBeenCalled();
});

it("preserves separate CLI and IDE sessions at the same path", async () => {
  const cli = { ...source, origin: "cli" };
  const ide = { ...source, source_id: "codex:ide", title: "IDE", origin: "vscode" };
  mocks.local.mockResolvedValue(okSources([cli, ide]));
  await mount();
  expect(row(cli.title).textContent).toContain("CLI");
  expect(row(ide.title).textContent).toContain("VS Code");
  await click(checkbox(cli.title));
  await click(checkbox(ide.title));
  await click(main(2));
  expect(mocks.join).toHaveBeenCalledExactlyOnceWith(group, [cli.source_id, ide.source_id]);
});

it.each([undefined, "future-origin"])(
  "allows an optional origin value %s without displaying a false badge",
  async (origin) => {
    mocks.local.mockResolvedValue({ ok: true, result: { sources: [{ ...source, origin }] } });
    await mount();
    expect(row(source.title).querySelector('[title^="Session created from"]')).toBeNull();
    expect(checkbox(source.title).disabled).toBe(false);
  },
);

it.each([null, group])(
  "prevents duplicate mutations and refreshes after closing target %s",
  async (target) => {
    const pending = defer();
    const mutate = target ? mocks.join : mocks.create;
    mutate.mockReturnValueOnce(pending.response);
    await mount(target);
    await click(checkbox(source.title));
    const submit = target ? main() : create();
    await act(async () => {
      submit.click();
      submit.click();
    });
    expect(submit.disabled).toBe(true);
    expect(submit.getAttribute("aria-busy")).toBe("true");
    expect(checkbox(source.title).disabled).toBe(true);
    expect(mutate).toHaveBeenCalledOnce();
    await click(button(t("common:close")));
    await act(async () =>
      pending.resolve({
        ok: true,
        result: target ? { group_id: group, group: existingGroup } : { group_id: "g-created" },
      }),
    );
    expect(mocks.refresh).toHaveBeenCalledOnce();
    expect(select).not.toHaveBeenCalled();
    await waitPoll();
    expect(mocks.local).toHaveBeenCalledOnce();
  },
);

it.each([null, group])(
  "reports a failed target %s write, preserving selection for an explicit retry",
  async (target) => {
    const mutate = target ? mocks.join : mocks.create;
    mutate.mockResolvedValueOnce({ ok: false, error: { message: "Fixture mutation failure" } });
    await mount(target);
    await click(checkbox(second.title));
    await click(checkbox(source.title));
    const action = target ? () => main(2) : create;
    await click(action());
    await waitPoll();
    expect(mocks.error).toHaveBeenCalledExactlyOnceWith("Fixture mutation failure");
    expect(mutate).toHaveBeenCalledOnce();
    expect(mocks.refresh).not.toHaveBeenCalled();
    expect(select).not.toHaveBeenCalled();
    expect(checkbox(source.title).checked).toBe(true);
    expect(checkbox(second.title).checked).toBe(true);
    await click(action());
    if (target)
      expect(mutate).toHaveBeenLastCalledWith(group, [second.source_id, source.source_id]);
    else expect(mutate).toHaveBeenLastCalledWith([second.source_id, source.source_id], "");
  },
);

it("retains terminal instructions, setup commands and the collapsed troubleshooting section", async () => {
  await mount();
  const details = Array.from(host.querySelectorAll("details")).find(
    (section) => section.querySelector("summary")?.textContent === enLayout.localSessions.setup,
  )!;
  expect(details.open).toBe(false);
  await click(details.querySelector("summary"));
  expect(details.open).toBe(true);
  expect(details.textContent).toContain("same OS user");
  expect(details.textContent).toContain("CCCC_HOME");
  expect(details.textContent).toContain("cccc sessions enable --runtime codex");
  expect(host.textContent).toContain("running cccc in a terminal, type /setup there");
  expect(host.textContent).toContain("reply through CCCC using Send");
  const troubleshooting = Array.from(host.querySelectorAll("details")).find(
    (section) =>
      section.querySelector("summary")?.textContent === enLayout.localSessions.troubleshooting,
  );
  expect(troubleshooting?.open).toBe(false);
  expect(troubleshooting?.textContent).toContain("cccc sessions list");
  expect(troubleshooting?.textContent).toContain("daemon_auto_start = true");
});

it.each(["en", "zh", "ja"])(
  "labels both the selected sessions and the target in %s",
  async (language) => {
    await i18n.changeLanguage(language);
    await mount();
    expect(button(t("localSessions.attachSelectedSessions")).disabled).toBe(true);
    await click(checkbox(source.title));
    await click(checkbox(second.title));
    expect(main(2).disabled).toBe(false);
    expect(main(2).textContent).not.toContain("attachSessions");
    expect(host.textContent).toContain(
      t("localSessions.targetGroupSummary", { group: existingGroup.title }),
    );
    expect(host.textContent).toContain(
      t("localSessions.selectedCountByComputer", { count: 2, localCount: 1, remoteCount: 1 }),
    );
  },
);
