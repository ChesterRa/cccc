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

it("keeps no sidebar entry and manages paired computers only in settings mode", async () => {
  const name = "Office Mac";
  const devices = [
    { node_id: "alpha", name, enabled: true, connected: false },
    { node_id: "beta", name, enabled: true, connected: true },
    { node_id: "revoked", name: "Studio", enabled: false, connected: true },
  ];
  const calls: string[] = [];
  let forbidden = false;
  let registered = false;
  vi.stubGlobal("fetch", async (input: unknown, init?: RequestInit) => {
    const path = String(input);
    calls.push((init?.method ?? "GET") + " " + path);
    if (path === apiRoot)
      return forbidden
        ? fail(403)
        : ok({
            supported: true,
            devices: registered ? devices : [],
            supported_runtimes: ["codex", "claude"],
          });
    if (path === apiRoot + "/beta/revoke" && init?.method === "POST") {
      devices[1] = { ...devices[1], enabled: false, connected: false };
      return ok({ revoked: true });
    }
    throw Error("Unexpected device operation");
  });
  const noop = () => {};
  useGroupStore.setState({ groups: [existingGroup] });
  // Session entry moved into a Group's Add Agent and Settings, so the sidebar offers none.
  await mount(
    <GroupSidebar
      orderedGroups={[existingGroup]}
      archivedGroupIds={[]}
      selectedGroupId={existingGroup.group_id}
      isOpen
      isCollapsed={false}
      sidebarWidth={248}
      isDark={false}
      onSelectGroup={noop}
      onClose={noop}
      onToggleCollapse={noop}
      onResizeWidth={noop}
      onReorderSection={noop}
      onArchiveGroup={noop}
      onRestoreGroup={noop}
    />,
  );
  // The former sidebar entry is gone; adding sessions now starts from a Group's Add Agent.
  expect(button("Connect sessions")).toBeUndefined();
  await refresh();
  expect(calls).toEqual([]);

  const rows = () => [...host.querySelectorAll(`ul[aria-label="${labels.devices}"] > li`)];
  const directoryReads = () => calls.filter((call) => call === "GET " + apiRoot).length;
  const setSearch = async (value: string) => {
    const search = host.querySelector<HTMLInputElement>('input[type="search"]')!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(
        search,
        value,
      );
      search.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  const settings = (active: boolean) =>
    mount(<SessionNodesPanel mode="settings" active={active} />);
  await settings(true);
  expect(directoryReads()).toBe(1);
  expect(host.textContent).toContain(labels.noPairedDevices);
  // Pairing stays folded in settings until asked for; the list and its management come first.
  expect(button(labels.create)).toBeUndefined();
  expect(button(labels.backToSessionList)).toBeUndefined();
  registered = true;
  await refresh();
  const [offline, online, revoked] = rows();
  expect(offline.textContent).toContain(labels.offline);
  expect(revoked.textContent).toContain(labels.revoked);
  expect(online.textContent).toContain("beta");
  await setSearch("  STU ");
  expect(rows()).toHaveLength(1);
  expect(rows()[0].textContent).toContain("Studio");
  await setSearch("missing-device");
  expect(host.textContent).toContain(labels.noMatchingDevices);
  await setSearch("office");
  expect(rows()).toHaveLength(2);
  await click(labels.revoke, rows()[1]);
  expect(rows()[1].textContent).toContain(labels.revokeConfirm);
  expect(calls.every((call) => call.startsWith("GET "))).toBe(true);
  await click(labels.revoke, rows()[1]);
  expect(rows()[1].textContent).toContain(labels.revoked);
  const pair = button(labels.pairComputer)!;
  expect(pair.getAttribute("aria-expanded")).toBe("false");
  await click(labels.pairComputer);
  expect(pair.getAttribute("aria-expanded")).toBe("true");
  expect(button(labels.create)).toBeDefined();
  expect(host.querySelector("pre")).not.toBeNull();
  forbidden = true;
  await refresh();
  expect(host.textContent).toContain(labels.adminRequired);
  expect(rows()).toHaveLength(0);
  expect(button(labels.create)).toBeUndefined();
  forbidden = false;
  await settings(false);
  const hidden = directoryReads();
  await refresh();
  await refresh();
  expect(directoryReads()).toBe(hidden);
  expect(calls.filter((call) => call.startsWith("POST "))).toEqual([
    "POST " + apiRoot + "/beta/revoke",
  ]);
});

it("shares one relay switch between pairing and settings, stops only from settings, and reads backend runtimes", async () => {
  history.replaceState(
    { retained: true },
    "",
    "/?group=fixture&session_node_join=private-fixture-id#chat",
  );
  const status = {
    supported: true,
    transport: "iroh",
    endpoint: {
      endpoint_id: "hub-1",
      addresses: ["127.0.0.1:4123"],
      relay_url: null as string | null,
    },
    relay: { enabled: false, effective: false, public_url: null as string | null },
    devices: [],
    supported_runtimes: ["codex"] as string[],
  };
  const calls: Array<{ method: string; path: string; body?: unknown }> = [];
  let issueCount = 0;
  const originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  const clipboard = vi.fn(async (_value: string) => {});
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: clipboard },
  });
  vi.stubGlobal("fetch", async (input: unknown, init?: RequestInit) => {
    const path = String(input);
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(String(init.body)) : undefined;
    calls.push({ method, path, body });
    if (path === apiRoot && method === "GET") return ok(status);
    if (path === apiRoot + "/relay" && method === "PUT") {
      status.relay = {
        enabled: Boolean(body?.enabled),
        effective: Boolean(body?.enabled && status.relay.public_url),
        public_url: status.relay.public_url,
      };
      if (status.endpoint)
        status.endpoint.relay_url = status.relay.effective ? status.relay.public_url : null;
      return ok({ relay: status.relay });
    }
    if (path === apiRoot + "/invitations" && method === "POST") {
      issueCount += 1;
      return ok({
        ticket: `synthetic-ticket-${issueCount}`,
        expires_at: new Date(Date.now() + 600000).toISOString(),
      });
    }
    if (path === apiRoot + "/stop" && method === "POST") {
      status.endpoint = null as unknown as typeof status.endpoint;
      status.relay.effective = false;
      return ok({ stopped: true });
    }
    if (path.startsWith("/api/v1/web_access/session")) return fail(401, "auth_required");
    throw Error(`Unexpected endpoint: ${method} ${path}`);
  });
  try {
    // Removed ?session_node_join query fallback no longer intercepts normal workbench entry.
    const initialize = vi.fn();
    await mount(<WebRoot pathname="/" onWorkbenchEntry={initialize} />);
    expect(initialize).toHaveBeenCalledOnce();
    expect(host.textContent).toContain("Fixture sign-in");

    await mount(<SessionNodesPanel mode="picker" onBackToSessions={vi.fn()} />);
    expect(button(labels.enableApp)).toBeDefined();
    await click(labels.enableApp);
    expect(
      [...document.querySelectorAll<HTMLElement>('[role="menuitemradio"]')].map(
        (item) => item.dataset.value,
      ),
    ).toEqual(["codex"]);
    await click(labels.enableApp);

    const relaySwitch = () =>
      host.querySelector<HTMLButtonElement>(
        `button[role="switch"][aria-label="${labels.publicRelayToggle}"]`,
      )!;
    expect(relaySwitch().getAttribute("aria-checked")).toBe("false");
    expect(relaySwitch().disabled).toBe(true);
    expect(host.textContent).toContain(labels.publicRelayRequiresRemoteAccess);

    status.relay = { enabled: false, effective: false, public_url: "https://hub.example.com/" };
    await refresh();
    expect(relaySwitch().disabled).toBe(false);
    expect(host.textContent).toContain("https://hub.example.com/");
    expect(host.textContent).toContain(labels.publicRelayOffHint);

    await click(labels.create);
    expect(clipboard).toHaveBeenLastCalledWith("synthetic-ticket-1");

    await act(async () => relaySwitch().click());
    expect(calls.at(-1)).toEqual({
      method: "PUT",
      path: apiRoot + "/relay",
      body: { enabled: true },
    });
    expect(relaySwitch().getAttribute("aria-checked")).toBe("true");
    expect(host.textContent).toContain(labels.publicRelayActiveHint);
    // Toggling relay invalidates any cached ticket so the next copy issues a fresh one.
    expect(button(labels.create)).toBeDefined();
    await click(labels.create);
    expect(clipboard).toHaveBeenLastCalledWith("synthetic-ticket-2");

    // Pairing in the session list offers the relay but leaves revoking and stopping to settings.
    expect(button(labels.stopGateway)).toBeUndefined();
    expect(button(labels.revoke)).toBeUndefined();
    expect(host.textContent).toContain(labels.manageInSettings);
    await mount(<SessionNodesPanel mode="settings" />);
    // Settings shows the same instance-wide relay switch, already on.
    expect(relaySwitch().getAttribute("aria-checked")).toBe("true");
    expect(button(labels.stopGateway)).toBeDefined();
    await click(labels.stopGateway);
    expect(calls.some((call) => call.method === "POST" && call.path === apiRoot + "/stop")).toBe(
      false,
    );
    expect(host.textContent).toContain(labels.stopGatewayConfirm);
    await click(enCommon.cancel);
    expect(host.textContent).not.toContain(labels.stopGatewayConfirm);
    expect(host.textContent).toContain(labels.gatewayRunning);

    await click(labels.stopGateway);
    await click(labels.stopGateway);
    expect(calls.some((call) => call.method === "POST" && call.path === apiRoot + "/stop")).toBe(
      true,
    );
    expect(button(labels.stopGateway)).toBeUndefined();
    expect(host.textContent).toContain(labels.publicRelayPendingHint);
    await click(labels.pairComputer);
    expect(button(labels.create)).toBeDefined();

    status.supported_runtimes = [];
    await refresh();
    expect(button(labels.enableApp)).toBeUndefined();
    expect(button(labels.create)?.disabled).toBe(true);
  } finally {
    if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
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
        supported_runtimes: ["codex", "claude"],
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
    const back = vi.fn();
    const picker = (active: boolean) =>
      mount(<SessionNodesPanel mode="picker" active={active} onBackToSessions={back} />);
    await picker(true);
    expect(host.textContent).toContain(labels.adminRequired);
    expect(button(labels.create)).toBeUndefined();
    // The way back to the session list stays available even without access.
    await click(labels.backToSessionList);
    expect(back).toHaveBeenCalledOnce();
    mode = "failure";
    await refresh();
    expect(host.textContent).toContain(labels.directoryFailed);
    expect(button(labels.create)).toBeUndefined();
    for (const state of ["absent", "unsupported"] as const) {
      mode = state;
      await refresh();
      expect(host.textContent).toContain(labels.upgradeRequired);
      expect(button(labels.create)).toBeUndefined();
      expect(button(labels.backToSessionList)).toBeDefined();
    }
    expect(posts).toBe(0);
    mode = "ready";
    await refresh();
    expect(host.textContent).not.toContain("Already paired");
    expect(button(labels.revoke)).toBeUndefined();
    await click(labels.create);
    expect(clipboard).toHaveBeenLastCalledWith(ticket);
    expect(host.textContent).not.toContain(ticket);
    expect(host.textContent).toContain(labels.ticketCopiedHint);
    // Copying is not pairing: no shortcut claims the computer's sessions are ready.
    expect(host.textContent).toContain(labels.ticketDirectoryHintPicker);
    // Hidden while the list shows, the ticket stays mounted but is not shown until access is rechecked.
    await picker(false);
    expect(button(enCommon.copied, host)).toBeUndefined();
    expect(host.textContent).not.toContain(ticket);
    expect(clipboard).toHaveBeenCalledTimes(1);
    expect(posts).toBe(1);
    await picker(true);
    await act(async () => vi.advanceTimersByTimeAsync(0));
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
