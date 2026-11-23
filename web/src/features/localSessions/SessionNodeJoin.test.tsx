// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import enLayout from "../../i18n/locales/en/layout.json";
import enCommon from "../../i18n/locales/en/common.json";
import { useGroupStore } from "../../stores";
import { existingGroup } from "./LocalSessionsButton.fixtures";
import { setAuthToken } from "../../services/api/base";
import { SessionNodesPanel } from "./SessionNodesPanel";
import { WebRoot } from "../../WebRoot";
import { GroupSidebar, type GroupSidebarProps } from "../../components/layout/GroupSidebar";

vi.mock("../../App", () => ({ default: () => <div>Fixture workbench</div> }));
vi.mock("../../components/auth/AuthTokenLoginForm", () => ({
  AuthTokenLoginForm: () => <div>Fixture sign-in</div>,
}));

const i18n = createInstance();
const labels = enLayout.sessionNodes;
const apiRoot = "/api/v1/session_nodes";
const ok = (result: unknown) => Response.json({ ok: true, result });
const fail = (status: number, code = "forbidden") =>
  Response.json({ ok: false, error: { code, message: "Fixture access failure" } }, { status });
const button = (label: string, within: ParentNode = document) =>
  [...within.querySelectorAll("button")].find(
    (el) =>
      !el.closest("[hidden]") &&
      (el.textContent === label || el.getAttribute("aria-label") === label),
  );
const click = async (label: string, within?: ParentNode) => {
  const element = button(label, within);
  expect(element, label).toBeDefined();
  await act(async () => element!.click());
};
let host: HTMLDivElement;
let root: Root;
let initialGroups: GroupSidebarProps["orderedGroups"];
const mount = (jsx: ReactNode) =>
  act(async () => root.render(<I18nextProvider i18n={i18n}>{jsx}</I18nextProvider>));
const refresh = () => act(async () => vi.advanceTimersByTimeAsync(5000));

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  initialGroups = useGroupStore.getState().groups;
  history.replaceState(null, "", "/");
  setAuthToken("");
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  await i18n
    .use(initReactI18next)
    .init({
      lng: "en",
      defaultNS: "layout",
      resources: { en: { layout: enLayout, common: enCommon } },
    });
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  useGroupStore.setState({ groups: initialGroups });
  history.replaceState(null, "", "/");
  setAuthToken("");
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("switches within the add-device dialog, filters sessions by device, and revokes only the selected computer", async () => {
  const name = "Office Mac";
  const otherGroup = { ...existingGroup, group_id: "g-alternate", title: "Alternate workspace" };
  useGroupStore.setState({ groups: [existingGroup, otherGroup] });
  const devices = [
    { node_id: "alpha", name, enabled: true, connected: false },
    { node_id: "beta", name, enabled: true, connected: true },
    { node_id: "revoked", name: "Studio", enabled: false, connected: true },
  ];
  const calls: string[] = [];
  let forbidden = false;
  let registered = false;
  let secondSession = false;
  let memberEnabled: boolean | undefined;
  const source = (node: string, title: string) => ({
    source_id: `remote:${node}:grant:source`,
    native_session_id: title,
    title,
    cwd: "/fixture",
    runtime: "codex",
    origin: "remote:" + name,
    connected: true,
  });
  vi.stubGlobal("fetch", async (input: unknown, init?: RequestInit) => {
    const path = String(input);
    calls.push((init?.method ?? "GET") + " " + path);
    if (path.startsWith("/api/v1/groups/") && path.endsWith("/actors")) {
      const member = {
        id: "fixture-member",
        external_session_id: source("beta", "Second workspace").source_id,
        enabled: memberEnabled,
      };
      return ok({
        actors:
          path === `/api/v1/groups/${existingGroup.group_id}/actors` && memberEnabled !== undefined
            ? [member]
            : [],
      });
    }
    if (path === apiRoot)
      return forbidden ? fail(403) : ok({ supported: true, devices: registered ? devices : [] });
    if (path === "/api/v1/local_sessions")
      return ok({
        sources: [
          source("alpha", "First workspace"),
          ...(secondSession ? [source("beta", "Second workspace")] : []),
        ],
      });
    if (path === apiRoot + "/beta/revoke" && init?.method === "POST") {
      devices[1] = { ...devices[1], enabled: false, connected: false };
      return ok({ revoked: true });
    }
    throw Error("Unexpected device operation");
  });
  const noop = () => {};
  const defaults: GroupSidebarProps = {
    orderedGroups: [existingGroup, otherGroup],
    archivedGroupIds: [],
    selectedGroupId: existingGroup.group_id,
    isOpen: true,
    isCollapsed: false,
    sidebarWidth: 248,
    isDark: false,
    onSelectGroup: noop,
    onClose: noop,
    onToggleCollapse: noop,
    onResizeWidth: noop,
    onReorderSection: noop,
    onArchiveGroup: noop,
    onRestoreGroup: noop,
  };
  const sidebar = (extra: Partial<GroupSidebarProps>) =>
    mount(<GroupSidebar {...defaults} {...extra} />);
  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]')!;
  const activeTab = (label: string) =>
    [...dialog().querySelectorAll<HTMLElement>('[role="tab"]')].find(
      (tab) => !tab.closest("[hidden]") && tab.textContent === label,
    )!;
  const visibleDevicePanel = () =>
    [...dialog().querySelectorAll<HTMLElement>('[role="tabpanel"]')].find(
      (panel) => !panel.closest("[hidden]"),
    )!;
  const rows = () => [
    ...visibleDevicePanel().querySelectorAll(`ul[aria-label="${labels.devices}"] > li`),
  ];
  const directoryReads = () => calls.filter((call) => call === "GET " + apiRoot).length;
  const setSearch = async (value: string) => {
    const search = visibleDevicePanel().querySelector<HTMLInputElement>('input[type="search"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
        search,
        value,
      );
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  await sidebar({ canAccessLocalSessions: true, readOnly: true });
  expect(button(enLayout.localSessions.connectShort)).toBeUndefined();
  await sidebar({ canAccessLocalSessions: true });
  expect(button(labels.devices, host)).toBeUndefined();
  const trigger = button(enLayout.localSessions.connectShort)!;
  await refresh();
  expect(calls).toEqual([]);
  trigger.focus();
  await click(enLayout.localSessions.connectShort);
  let original = dialog();
  expect(directoryReads()).toBe(0);
  const firstChoice = () =>
    [...original.querySelectorAll("label")]
      .find((label) => label.textContent?.includes("First workspace"))
      ?.querySelector<HTMLInputElement>('input[type="checkbox"]');
  await act(async () => firstChoice()!.click());
  expect(firstChoice()?.checked).toBe(true);
  await click(enLayout.localSessions.addDevice, original);
  expect(dialog()).toBe(original);
  expect(activeTab(enLayout.localSessions.addDevice)?.getAttribute("aria-selected")).toBe("true");
  expect(visibleDevicePanel().querySelector("pre")).not.toBeNull();
  expect(directoryReads()).toBe(1);
  await act(async () => {
    activeTab(enLayout.localSessions.addDevice).dispatchEvent(
      new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
    );
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(32);
  });
  expect(activeTab(labels.devices).getAttribute("aria-selected")).toBe("true");
  expect(document.activeElement).toBe(activeTab(labels.devices));
  expect(visibleDevicePanel().querySelector("pre")).toBeNull();
  expect(button(labels.create, original)).toBeUndefined();
  expect(visibleDevicePanel().textContent).toContain(labels.noPairedDevices);
  expect(directoryReads()).toBe(1);
  registered = true;
  await refresh();
  const [offline, online, revoked] = rows();
  expect(offline.textContent).toContain(labels.offline);
  expect(button(labels.chooseSessionsToJoin, offline)).toBeUndefined();
  expect(revoked.textContent).toContain(labels.revoked);
  expect(button(labels.chooseSessionsToJoin, revoked)).toBeUndefined();
  expect(online.textContent).toContain("beta");
  await setSearch("  STU ");
  expect(rows()).toHaveLength(1);
  expect(rows()[0].textContent).toContain("Studio");
  await setSearch("missing-device");
  expect(dialog().textContent).toContain(labels.noMatchingDevices);
  await setSearch("office");
  expect(rows()).toHaveLength(2);
  await click(labels.chooseSessionsToJoin, rows()[1]);
  expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  expect(dialog().textContent).toContain(i18n.t("sessionNodes.noDeviceSessions", { device: name }));
  expect(dialog().textContent).not.toContain("First workspace");
  const beforeSourceRefresh = directoryReads();
  secondSession = true;
  await act(async () => vi.advanceTimersByTimeAsync(5000));
  expect(directoryReads()).toBe(beforeSourceRefresh);
  expect(dialog().textContent).toContain("Second workspace");
  expect(dialog().textContent).not.toContain("First workspace");
  expect(dialog().textContent).toContain(
    i18n.t("sessionNodes.deviceSessionsHint", { action: enLayout.localSessions.continue }),
  );
  const enabledSession = [...original.querySelectorAll("label")]
    .find((label) => label.textContent?.includes("Second workspace"))!
    .querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  await act(async () => enabledSession.click());
  await click(enLayout.localSessions.continue, original);
  const target = () =>
    original.querySelector<HTMLButtonElement>(
      `section[aria-label="${enLayout.localSessions.target}"] button[aria-haspopup="menu"]`,
    )!;
  expect(target().dataset.value).toBe(existingGroup.group_id);
  const selection = original.querySelector(
    `section[aria-label="${enLayout.localSessions.selectedSessions}"]`,
  );
  expect(selection?.textContent).toContain("Second workspace");
  expect(selection?.textContent).not.toContain("First workspace");
  await act(async () => target().click());
  const alternate = document.querySelector<HTMLElement>(
    `[role="menuitemradio"][data-value="${otherGroup.group_id}"]`,
  )!;
  await act(async () => alternate.click());
  expect(target().dataset.value).toBe(otherGroup.group_id);
  expect(button(enLayout.localSessions.join, original)?.disabled).toBe(false);
  expect(calls.every((call) => call.startsWith("GET "))).toBe(true);
  await click(enLayout.localSessions.backToSessions, original);
  await click(labels.allSessions, dialog());
  expect(dialog().textContent).toContain("First workspace");
  expect(firstChoice()?.checked).toBe(true);
  await click(labels.devices, dialog());
  expect(visibleDevicePanel().querySelector<HTMLInputElement>('input[type="search"]')?.value).toBe(
    "office",
  );
  // On a later visit the current Group already owns this source. Looking at it only reads the roster.
  await click(enCommon.close, original);
  memberEnabled = true;
  const rosterReads = () =>
    calls.filter((call) => call === `GET /api/v1/groups/${existingGroup.group_id}/actors`).length;
  const beforeReopen = rosterReads();
  await click(enLayout.localSessions.connectShort);
  original = dialog();
  expect(rosterReads()).toBe(beforeReopen + 1);
  await click(enLayout.localSessions.addDevice, original);
  await click(labels.devices, original);
  expect(rosterReads()).toBe(beforeReopen + 1);
  await click(labels.chooseSessionsToJoin, rows()[1]);
  const memberRow = () =>
    [...original.querySelectorAll("label")].find((label) =>
      label.textContent?.includes("Second workspace"),
    );
  expect(memberRow()?.textContent).toContain(enLayout.localSessions.memberJoined);
  expect(rosterReads()).toBe(beforeReopen + 2);
  expect(original.textContent).not.toContain(
    i18n.t("sessionNodes.deviceSessionsHint", { action: enLayout.localSessions.continue }),
  );
  expect(original.textContent).not.toContain(
    i18n.t("sessionNodes.noDeviceSessions", { device: name }),
  );
  memberEnabled = false;
  await act(async () => vi.advanceTimersByTimeAsync(3000));
  expect(memberRow()?.textContent).toContain(enLayout.localSessions.memberPaused);
  expect(button(enLayout.localSessions.continue, original)?.disabled).toBe(true);
  expect(calls.every((call) => call.startsWith("GET "))).toBe(true);
  await click(labels.devices, original);
  await click(labels.revoke, rows()[1]);
  expect(rows()[1].textContent).toContain(labels.revokeConfirm);
  expect(calls.every((call) => call.startsWith("GET "))).toBe(true);
  await click(labels.revoke, rows()[1]);
  expect(rows()[1].textContent).toContain(labels.revoked);
  expect(button(labels.chooseSessionsToJoin, rows()[1])).toBeUndefined();
  devices[0] = { ...devices[0], connected: true };
  await refresh();
  expect(button(labels.chooseSessionsToJoin, rows()[0])).toBeDefined();
  forbidden = true;
  await refresh();
  expect(dialog().textContent).toContain(labels.adminRequired);
  expect(original.querySelector('[role="tablist"]')).toBeNull();
  expect(button(labels.chooseSessionsToJoin, original)).toBeUndefined();
  expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
  await act(async () => {
    document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  });
  await act(async () => {
    await vi.advanceTimersByTimeAsync(32);
  });
  expect(dialog()).toBeNull();
  expect(document.activeElement).toBe(trigger);
  const afterClose = directoryReads();
  await refresh();
  expect(directoryReads()).toBe(afterClose);
  expect(calls.filter((call) => call.startsWith("POST "))).toEqual([
    "POST " + apiRoot + "/beta/revoke",
  ]);
});

it("shows terminal-pairing instructions for old links without loading authentication or device APIs", async () => {
  history.replaceState(
    { retained: true },
    "",
    "/?group=fixture&session_node_join=private-fixture-id#chat",
  );
  const calls: string[] = [];
  vi.stubGlobal("fetch", async (input: unknown) => {
    const path = String(input);
    calls.push(path);
    if (path.startsWith("/api/v1/web_access/session")) return fail(401, "auth_required");
    throw Error("Unexpected legacy-link request");
  });
  const initialize = vi.fn();
  await mount(<WebRoot pathname="/" onWorkbenchEntry={initialize} />);
  expect(host.textContent).toContain(labels.oldLinkHelp);
  expect(host.textContent).not.toContain("private-fixture-id");
  expect(host.textContent).not.toContain("Fixture sign-in");
  await act(async () => vi.advanceTimersByTimeAsync(16000));
  history.pushState(
    { retained: true },
    "",
    "/?group=fixture&session_node_join=&section=devices#chat",
  );
  await act(async () => {
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  expect(host.textContent).toContain(labels.oldLinkTitle);
  expect(calls).toEqual([]);
  expect(initialize).not.toHaveBeenCalled();
  expect(host.querySelector("button, a, input, form")).toBeNull();
  expect(location.search).toBe("?group=fixture&session_node_join=&section=devices");
  expect(location.hash).toBe("#chat");
  expect(history.state).toEqual({ retained: true });
  // Independently visiting a normal workbench URL retains its ordinary authentication flow.
  history.pushState({ retained: true }, "", "/?group=fixture&section=devices#chat");
  await act(async () => {
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  expect(initialize).toHaveBeenCalledOnce();
  expect(location.search).toBe("?group=fixture&section=devices");
  expect(location.hash).toBe("#chat");
  expect(history.state).toEqual({ retained: true });
  expect(host.textContent).toContain("Fixture sign-in");
  expect(calls.length).toBeGreaterThan(0);
  expect(calls.every((path) => path.startsWith("/api/v1/web_access/session"))).toBe(true);
});

it("offers tickets only with verified access and clears a cached ticket after access is lost", async () => {
  let mode: "forbidden" | "failure" | "absent" | "unsupported" | "ready" = "forbidden";
  let posts = 0;
  const ticket = "synthetic-private-device-ticket";
  const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  const clipboard = vi.fn(async (_value: string) => {});
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: clipboard },
  });
  vi.stubGlobal("fetch", async (input: unknown, init?: RequestInit) => {
    const path = String(input);
    if (path === apiRoot) {
      if (mode === "forbidden") return fail(403);
      if (mode === "absent") return fail(404, "not_found");
      if (mode === "failure") throw Error("Fixture network unavailable");
      return ok({
        supported: mode === "ready",
        devices: [{ node_id: "elsewhere", name: "Already paired", enabled: true, connected: true }],
      });
    }
    if (path === apiRoot + "/invitations" && init?.method === "POST") {
      posts++;
      return posts === 1
        ? ok({ ticket, expires_at: new Date(Date.now() + 600000).toISOString() })
        : fail(403);
    }
    throw Error("Unexpected endpoint");
  });
  try {
    await mount(<SessionNodesPanel onChooseSessions={vi.fn()} />);
    expect(host.textContent).toContain(labels.adminRequired);
    expect(button(labels.create)).toBeUndefined();
    expect(button(labels.selectDeviceSessions)).toBeUndefined();
    mode = "failure";
    await refresh();
    expect(host.textContent).toContain(labels.directoryFailed);
    expect(button(labels.create)).toBeUndefined();
    for (const state of ["absent", "unsupported"] as const) {
      mode = state;
      await refresh();
      expect(host.textContent).toContain(labels.upgradeRequired);
      expect(button(labels.create)).toBeUndefined();
    }
    expect(posts).toBe(0);
    mode = "ready";
    await refresh();
    expect(host.textContent).not.toContain("Already paired");
    expect(button(labels.chooseSessionsToJoin)).toBeUndefined();
    expect(button(labels.revoke)).toBeUndefined();
    await click(labels.create);
    expect(clipboard).toHaveBeenLastCalledWith(ticket);
    expect(host.textContent).not.toContain(ticket);
    expect(host.textContent).toContain(labels.ticketCopiedHint);
    await click(labels.selectDeviceSessions, host);
    const manager = host.querySelector<HTMLElement>('[role="tabpanel"]:not([hidden])');
    expect(manager?.textContent).toContain("Already paired");
    expect(host.textContent).not.toContain(ticket);
    expect(button(labels.create, host)).toBeUndefined();
    expect(button(enCommon.copied, host)).toBeUndefined();
    expect(clipboard).toHaveBeenCalledTimes(1);
    expect(posts).toBe(1);
    await click(enLayout.localSessions.addDevice, host);
    expect(button(enCommon.copied, host)).toBeDefined();
    expect(clipboard).toHaveBeenCalledTimes(1);
    expect(posts).toBe(1);
    mode = "failure";
    await refresh();
    expect(button(labels.copyTicket)).toBeUndefined();
    expect(button(enCommon.copied)).toBeUndefined();
    mode = "ready";
    await refresh();
    expect(button(enCommon.copied)).toBeDefined();
    await click(enCommon.copied);
    expect(clipboard).toHaveBeenCalledTimes(2);
    expect(clipboard).toHaveBeenLastCalledWith(ticket);
    expect(posts).toBe(1);
    await click(labels.createAnother);
    expect(host.textContent).toContain(labels.adminRequired);
    expect(button(labels.copyTicket)).toBeUndefined();
    expect(button(enCommon.copied)).toBeUndefined();
    await refresh();
    expect(button(labels.create)).toBeDefined();
    expect(button(labels.copyTicket)).toBeUndefined();
    expect(button(enCommon.copied)).toBeUndefined();
    expect(posts).toBe(2);
  } finally {
    if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});
