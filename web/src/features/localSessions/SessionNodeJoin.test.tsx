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
  AuthTokenLoginForm: ({ onSubmit }: { onSubmit: (token: string) => void }) => (
    <button onClick={() => onSubmit("test-admin")}>Fixture administrator sign-in</button>
  ),
}));

const i18n = createInstance();
const labels = enLayout.sessionNodes;
let host: HTMLDivElement;
let root: Root;
let originalClipboard: PropertyDescriptor | undefined;
const copied = vi.fn(async (_text: string) => {});
const apiRoot = "/api/v1/session_nodes";
const single = apiRoot + "/join/requests/";
const pending = apiRoot + "/join/pending";
const ok = (result: unknown) => Response.json({ ok: true, result });
const request = (id: string, name: string, code: string) => ({
  request_id: id,
  name,
  node_id: `node-${id}`,
  user_code: code,
  expires_at: new Date(Date.now() + 60_000).toISOString(),
});
const button = (label: string) =>
  [...document.querySelectorAll("button")].find(
    (el) => el.textContent === label || el.getAttribute("aria-label") === label,
  );
const click = async (label: string) => {
  const target = button(label);
  expect(target).toBeDefined();
  await act(async () => target!.click());
};
const mount = (element: ReactNode) =>
  act(async () => root.render(<I18nextProvider i18n={i18n}>{element}</I18nextProvider>));

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  history.replaceState(null, "", "/");
  setAuthToken("");
  copied.mockClear();
  originalClipboard = Object.getOwnPropertyDescriptor(navigator, "clipboard");
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: copied },
  });
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
  if (originalClipboard) Object.defineProperty(navigator, "clipboard", originalClipboard);
  else Reflect.deleteProperty(navigator, "clipboard");
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("shows new arrivals on the open device panel and allows only the explicitly reviewed device", async () => {
  const first = request("alpha", "Office Mac", "A431B282");
  const second = request("beta", first.name, "BD912544");
  const rows = [first];
  const posts: string[] = [];
  let advertised: string | null = null;
  let reads = 0;
  let finish!: (response: Response) => void;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: unknown, init?: RequestInit) => {
      const path = String(input);
      if (init?.method === "POST") {
        posts.push(path);
        return new Promise<Response>((resolve) => {
          finish = resolve;
        });
      }
      if (path === apiRoot)
        return ok({ supported: true, join_supported: true, join_url: advertised, devices: [] });
      if (path === pending) {
        reads++;
        return ok({ requests: rows.slice() });
      }
      if (path === single + second.request_id)
        return ok({ request: { ...second, status: "pending" } });
      throw Error("unexpected request");
    }),
  );
  await mount(<SessionNodesPanel onChooseSessions={vi.fn()} />);
  expect(host.textContent).toContain(labels.joinNoAddress);
  expect(host.textContent).toContain(labels.joinTicketFallback);
  expect(button(labels.joinCopy)).toBeUndefined();
  expect(host.textContent).toContain(first.name);
  rows.push(second);
  advertised = "https://hub.example";
  await act(async () => {
    await vi.advanceTimersByTimeAsync(5000);
  });
  await click(labels.joinCopy);
  expect(copied.mock.calls).toEqual([["cccc sessions node join https://hub.example"]]);
  const matches = [...host.querySelectorAll("li")].filter((row) =>
    row.textContent?.includes(first.name),
  );
  expect(matches).toHaveLength(2);
  expect(matches.map((row) => row.querySelector("code")?.textContent)).toEqual([
    first.user_code,
    second.user_code,
  ]);
  const reviewLabel = i18n.t("sessionNodes.joinReviewDevice", {
    name: second.name,
    code: second.user_code,
  });
  expect(matches[1].contains(button(reviewLabel) ?? null)).toBe(true);
  await click(reviewLabel);
  const review = document.querySelector('[role="dialog"]');
  expect(review?.textContent).toContain(second.user_code);
  expect(review?.textContent).not.toContain(first.user_code);
  expect(posts).toEqual([]);
  const allow = button(labels.joinAllow)!;
  await act(async () => {
    allow.click();
    allow.click();
  });
  expect(posts).toEqual([single + second.request_id + "/approve"]);
  await act(async () => finish(ok({ status: "approved" })));
  expect(document.body.textContent).toContain(labels.joinApproved);
  expect(button(labels.joinAllow)).toBeUndefined();
  await mount(<div />);
  const atClose = reads;
  await act(async () => {
    await vi.advanceTimersByTimeAsync(12_000);
  });
  expect(reads).toBe(atClose);
});

it("resumes only the linked request after administrator login and keeps rejection explicit", async () => {
  history.replaceState({ retained: true }, "", "/?group=g-fixture&session_node_join=beta#chat");
  const second = request("beta", "Design PC", "BD912544");
  // This administrator's clock is ahead of the hub, which still reports the request as pending.
  second.expires_at = new Date(Date.now() - 60_000).toISOString();
  const requests: string[] = [];
  let admin = false;
  let status = "pending";
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: unknown, init?: RequestInit) => {
      const path = String(input);
      if (path.startsWith("/api/v1/web_access/session"))
        return ok({ web_access_session: { is_admin: admin, current_browser_signed_in: admin } });
      if (path.startsWith("/api/v1/groups")) return ok({ groups: [] });
      requests.push(`${init?.method ?? "GET"} ${path}`);
      if (path === single + "beta") return ok({ request: { ...second, status } });
      if (path === single + "beta/deny") {
        status = "denied";
        throw Error("decision acknowledgement was lost");
      }
      throw Error("unexpected join API");
    }),
  );
  await mount(<WebRoot pathname="/" />);
  expect(requests).toEqual([]);
  expect(document.body.textContent).not.toContain(second.user_code);
  admin = true;
  await click("Fixture administrator sign-in");
  expect(requests).toEqual(["GET " + single + "beta"]);
  expect(document.body.textContent).toContain(second.user_code);
  await click(labels.joinDeny);
  expect(requests).toEqual([
    "GET " + single + "beta",
    "POST " + single + "beta/deny",
    "GET " + single + "beta",
  ]);
  expect(document.body.textContent).toContain(labels.joinDenied);
  await click(enCommon.close);
  expect(location.search).toBe("?group=g-fixture");
  expect(location.hash).toBe("#chat");
  expect(document.body.textContent).toContain("Fixture workbench");
});

it("keeps unavailable, expired and forbidden requests read-only", async () => {
  history.replaceState(null, "", "/?session_node_join=gone");
  const get = vi.fn(async (input: unknown, init?: RequestInit) => {
    const path = String(input);
    if (init?.method === "POST") throw Error("a terminal request cannot be decided");
    if (path.startsWith("/api/v1/web_access/session"))
      return ok({ web_access_session: { is_admin: true, current_browser_signed_in: true } });
    if (path.startsWith("/api/v1/groups")) return ok({ groups: [] });
    if (path === single + "forbidden")
      return Response.json(
        { ok: false, error: { code: "forbidden", message: "admin only" } },
        { status: 403 },
      );
    if (path === single + "gone")
      return Response.json(
        { ok: false, error: { code: "not_found", message: "missing" } },
        { status: 404 },
      );
    if (path === single + "expired")
      return ok({
        request: { ...request("expired", "Old device", "000AB001"), status: "expired" },
      });
    throw Error("unexpected join API");
  });
  vi.stubGlobal("fetch", get);
  await mount(<WebRoot pathname="/" />);
  expect(document.body.textContent).toContain(labels.joinUnavailable);
  history.pushState(null, "", "/?session_node_join=expired");
  await act(async () => {
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  expect(document.body.textContent).toContain(labels.joinExpired);
  history.pushState(null, "", "/?session_node_join=forbidden");
  await act(async () => {
    window.dispatchEvent(new PopStateEvent("popstate"));
  });
  expect(document.body.textContent).toContain(labels.joinForbidden);

  expect(button(labels.joinAllow)).toBeUndefined();
  expect(button(labels.joinDeny)).toBeUndefined();
  expect(
    get.mock.calls.some(([, init]) => (init as RequestInit | undefined)?.method === "POST"),
  ).toBe(false);
});
