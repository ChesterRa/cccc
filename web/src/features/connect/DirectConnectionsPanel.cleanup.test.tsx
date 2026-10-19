// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { DirectConnectionsPanel } from "./DirectConnectionsPanel";
import type { DirectRelation } from "./directConnectionModel";

const mocks = vi.hoisted(() => ({ request: vi.fn() }));
vi.mock("../../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));
let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
let relations: DirectRelation[];
const relation = (id: string, extra: Partial<DirectRelation> = {}): DirectRelation => ({
  id,
  local: { instance_id: "local", name: "Local", group_id: "group-a", title: "Local" },
  remote: null,
  state: "active",
  initiated: false,
  current: true,
  expired: false,
  expires_at: "2099-01-01T00:00:00Z",
  online: false,
  error: null,
  ...extra,
});
const posts = () =>
  mocks.request.mock.calls
    .filter(([, init]) => init?.method === "POST")
    .map(([, init]) => JSON.parse(init.body));
const render = () => act(async () => root.render(<DirectConnectionsPanel groupId="group-a" />));
const click = (label: string) =>
  act(async () => {
    const button = [...host.querySelectorAll("button")].find((b) => b.textContent === label);
    expect(button).toBeDefined();
    button!.click();
  });
beforeEach(() => {
  vi.useFakeTimers();
  relations = [];
  mocks.request.mockReset().mockImplementation(async (_url, init) => {
    if (init?.method === "POST") {
      const body = JSON.parse(init.body);
      if (body.action === "revoke")
        relations = relations.map((r) => (r.id === body.id ? { ...r, state: "revoked" } : r));
      if (body.action === "remove") relations = relations.filter((r) => r.id !== body.id);
      return { ok: true, result: {} };
    }
    return { ok: true, result: { listener: null, runtime: null, relations, addresses: [] } };
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("disconnects and removes the confirmed connection without a second click", async () => {
  relations = [relation("active")];
  await render();
  await click("direct.disconnect");
  expect(posts()).toEqual([]);
  await click("direct.confirm");
  expect(posts()).toEqual([
    { action: "revoke", group_id: "group-a", id: "active" },
    { action: "remove", group_id: "group-a", id: "active" },
  ]);
  expect(relations).toEqual([]);
  expect(host.querySelector("li")).toBeNull();
  expect(host.textContent).not.toContain("direct.history");
});

it("cleans existing closed records while retaining offline and uncertain connections", async () => {
  relations = [
    relation("revoked", { state: "revoked" }),
    relation("expired", { state: "expired" }),
    relation("offline"),
    relation("awaiting", { state: "pending", expired: true, initiated: true }),
  ];
  await render();
  expect(posts().map((p) => p.id)).toEqual(["revoked", "expired"]);
  expect(relations.map((r) => r.id)).toEqual(["offline", "awaiting"]);
  expect(host.textContent).toContain("direct.states.active");
  expect(host.textContent).toContain("direct.states.checkingApproval");
});

it("never restores closed rows or Remove UI when background cleanup fails", async () => {
  relations = [relation("closed", { state: "revoked" })];
  const normal = mocks.request.getMockImplementation()!;
  mocks.request.mockImplementation(async (url, init) =>
    init?.method === "POST"
      ? { ok: false, error: { code: "io_error", message: "Cleanup failed" } }
      : normal(url, init),
  );
  await render();
  expect(host.querySelector("li")).toBeNull();
  expect(host.textContent).not.toContain("direct.history");
  expect(host.textContent).not.toContain("direct.remove");
  expect(host.querySelector('[role="alert"]')).toBeNull();
  await act(async () => {
    await vi.advanceTimersByTimeAsync(9000);
  });
  expect(posts()).toHaveLength(1);
  expect(host.querySelector("li")).toBeNull();
  expect(relations).toHaveLength(1); // A failed backend removal is not claimed as successful.
});

it("does not remove an active connection when revocation fails", async () => {
  relations = [relation("active")];
  const normal = mocks.request.getMockImplementation()!;
  mocks.request.mockImplementation(async (url, init) =>
    init?.method === "POST"
      ? { ok: false, error: { code: "io_error", message: "Disconnect failed" } }
      : normal(url, init),
  );
  await render();
  await click("direct.disconnect");
  await click("direct.confirm");
  expect(posts()).toEqual([{ action: "revoke", group_id: "group-a", id: "active" }]);
  expect(relations[0].state).toBe("active");
  expect(host.textContent).toContain("Disconnect failed");
});

it("does not start cleanup from a status response after the panel is closed", async () => {
  let finish: (value: unknown) => void = () => {};
  mocks.request.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await render();
  const signal = mocks.request.mock.calls[0][1].signal as AbortSignal;
  await act(async () => root.render(null));
  expect(signal.aborted).toBe(true);
  await act(async () =>
    finish({
      ok: true,
      result: {
        relations: [relation("closed", { state: "revoked" })],
        listener: null,
        runtime: null,
        addresses: [],
      },
    }),
  );
  expect(posts()).toEqual([]);
});

it("hides closed rows while cleanup is pending and stops the batch on panel close", async () => {
  relations = [relation("first", { state: "revoked" }), relation("second", { state: "expired" })];
  let finish: (value: unknown) => void = () => {};
  const normal = mocks.request.getMockImplementation()!;
  mocks.request.mockImplementation((url, init) =>
    init?.method === "POST"
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : normal(url, init),
  );
  await render();
  expect(host.querySelector("li")).toBeNull();
  expect(posts()).toEqual([{ action: "remove", group_id: "group-a", id: "first" }]);
  const removal = mocks.request.mock.calls.find(([, init]) => init?.method === "POST")!;
  await act(async () => root.render(null));
  expect((removal[1].signal as AbortSignal).aborted).toBe(true);
  await act(async () => finish({ ok: true, result: {} }));
  await act(async () => {
    await vi.advanceTimersByTimeAsync(15000);
  });
  expect(posts()).toHaveLength(1);
});
