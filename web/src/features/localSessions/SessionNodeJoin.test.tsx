// @vitest-environment happy-dom
import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import enLayout from "../../i18n/locales/en/layout.json";
import enCommon from "../../i18n/locales/en/common.json";
import { setAuthToken } from "../../services/api/base";
import { SessionNodesPanel } from "./SessionNodesPanel";
import { WebRoot } from "../../WebRoot";

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
    (el) => el.textContent === label || el.getAttribute("aria-label") === label,
  );
const click = async (label: string, within?: ParentNode) => {
  const element = button(label, within);
  expect(element, label).toBeDefined();
  await act(async () => element!.click());
};
let host: HTMLDivElement;
let root: Root;
const mount = (jsx: ReactNode) =>
  act(async () => root.render(<I18nextProvider i18n={i18n}>{jsx}</I18nextProvider>));
const refresh = () => act(async () => vi.advanceTimersByTimeAsync(5000));

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
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
  history.replaceState(null, "", "/");
  setAuthToken("");
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("uses the device directory for online sessions and revokes only the selected computer", async () => {
  const name = "Office Mac";
  const devices = [
    { node_id: "alpha", name, enabled: true, connected: false },
    { node_id: "beta", name, enabled: true, connected: true },
    { node_id: "revoked", name: "Revoked", enabled: false, connected: true },
  ];
  const calls: string[] = [];
  vi.stubGlobal("fetch", async (input: unknown, init?: RequestInit) => {
    const path = String(input);
    calls.push((init?.method ?? "GET") + " " + path);
    if (path === apiRoot)
      return ok({
        supported: true,
        devices,
        join_supported: true,
        join_url: "https://retired.example",
      });
    if (path === apiRoot + "/beta/revoke" && init?.method === "POST") {
      devices[1] = { ...devices[1], enabled: false, connected: false };
      return ok({ revoked: true });
    }
    throw Error("Unexpected device operation");
  });
  const choose = vi.fn();
  await mount(<SessionNodesPanel onChooseSessions={choose} />);
  expect(calls).toEqual(["GET " + apiRoot]);
  expect(host.querySelector("pre")?.textContent).toBe(
    "cccc sessions node connect --runtime codex --ticket-stdin",
  );
  await click(labels.ticketManage);
  const rows = () => [...host.querySelectorAll(`ul[aria-label="${labels.devices}"] > li`)];
  const [offline, online, revoked] = rows();
  expect(offline.textContent).toContain(labels.offline);
  expect(button(labels.viewSessions, offline)).toBeUndefined();
  expect(button(labels.viewSessions, revoked)).toBeUndefined();
  await click(labels.viewSessions, online);
  expect(choose).toHaveBeenCalledOnce();
  await click(labels.revoke, online);
  expect(host.textContent).toContain(labels.revokeConfirm);
  expect(calls).toEqual(["GET " + apiRoot]);
  await click(labels.revoke, online);
  expect(rows()[1].textContent).toContain(labels.revoked);
  expect(button(labels.viewSessions, rows()[1])).toBeUndefined();
  devices[0] = { ...devices[0], connected: true };
  await refresh();
  expect(button(labels.viewSessions, rows()[0])).toBeDefined();
  expect(calls).toEqual(["GET " + apiRoot, "POST " + apiRoot + "/beta/revoke", "GET " + apiRoot]);
  await mount(<div />);
  await refresh();
  expect(calls).toHaveLength(3);
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
      return ok({ supported: mode === "ready", devices: [] });
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
    await click(labels.create);
    expect(clipboard).toHaveBeenLastCalledWith(ticket);
    expect(host.textContent).not.toContain(ticket);
    mode = "failure";
    await refresh();
    expect(button(labels.copyTicket)).toBeUndefined();
    mode = "ready";
    await refresh();
    await click(labels.copyTicket);
    expect(posts).toBe(1);
    await click(labels.createAnother);
    expect(host.textContent).toContain(labels.adminRequired);
    expect(button(labels.copyTicket)).toBeUndefined();
    await refresh();
    expect(button(labels.create)).toBeDefined();
    expect(button(labels.copyTicket)).toBeUndefined();
    expect(posts).toBe(2);
  } finally {
    if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
    else Reflect.deleteProperty(navigator, "clipboard");
  }
});
