// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { GroupConnectionsControl, GroupConnectionsPanel } from "./GroupConnectionsControl";
import { useModalStore } from "../../stores/useModalStore";
import type { GroupMeta } from "../../types";

const mocks = vi.hoisted(() => ({ request: vi.fn(), t: (key: string) => key }));
vi.mock("../../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: mocks.t, i18n: { language: "ja", resolvedLanguage: "ja" } }),
}));
const groups = [
  { group_id: "a", title: "Local A" },
  { group_id: "b", title: "Local B" },
] as GroupMeta[];
const status = {
  status: "ready",
  error_code: null,
  error_message: null,
  checked_at: new Date().toISOString(),
  links: [],
  expires_at: new Date(Date.now() + 120000).toISOString(),
  account_id: "member",
  account_origin: "https://account.test",
};
let root: ReturnType<typeof createRoot>, host: HTMLDivElement;
const buttons = () => [...document.querySelectorAll("button")];
const button = (key: string) =>
  buttons().find((b) => b.textContent === key || b.getAttribute("aria-label") === key)!;
const openConnections = (groupId = "a") =>
  act(async () => useModalStore.getState().setGroupConnections(groupId));
const groupSelect = () => document.querySelector<HTMLButtonElement>("[data-connect-group-select]")!;
/** The Group list is the shared dropdown now: open the menu, then pick the option. */
async function chooseGroup(groupId: string) {
  await act(async () => groupSelect().click());
  await act(async () =>
    document
      .querySelector<HTMLButtonElement>(`[role="menuitemradio"][data-value="${groupId}"]`)!
      .click(),
  );
}
async function render(enabled = true, groupId = "a") {
  await act(async () =>
    root.render(
      <GroupConnectionsControl
        enabled={enabled}
        groupId={groupId}
        groups={groups}
        onOpenAccount={() => {}}
      />,
    ),
  );
}
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  window.history.replaceState(null, "", "/");
  useModalStore.setState({ groupConnectionsId: null });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  mocks.request.mockReset().mockResolvedValue({ ok: true, result: status });
  vi.spyOn(window, "open").mockReturnValue(null);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.restoreAllMocks();
});

it("has no polling or management entry for restricted access and only polls while open", async () => {
  await render(false);
  expect(buttons()).toHaveLength(0);
  expect(mocks.request).not.toHaveBeenCalled();
  await render();
  expect(mocks.request).not.toHaveBeenCalled();
  await openConnections();
  expect(mocks.request).toHaveBeenCalledWith(
    "/api/v1/connect/groups?group_id=a",
    expect.objectContaining({ signal: expect.any(AbortSignal) }),
  );
  expect(mocks.request.mock.calls.every(([, options]) => options.method !== "POST")).toBe(true);
});

it("uses the explicitly selected Group for an incoming invitation and leaves approval to the account website", async () => {
  const invitation = "11111111-1111-4111-8111-111111111111";
  window.history.replaceState(null, "", `/?connect_invite=${invitation}`);
  await render();
  expect(document.querySelector('[role="dialog"]')).not.toBeNull();
  await chooseGroup("b");
  const url = "https://account.test/connect/select?ticket=fixture";
  mocks.request.mockResolvedValueOnce({ ok: true, result: { url } });
  await act(async () => button("groupConnections.accept").click());
  const call = mocks.request.mock.calls.find(([, options]) => options.method === "POST")!;
  expect(JSON.parse(call[1].body)).toEqual({ group_id: "b", invitation });
  expect([...document.querySelectorAll("a")].some((a) => a.href === `${url}&lang=ja`)).toBe(true);
  expect(mocks.request.mock.calls.filter(([, options]) => options.method === "POST")).toHaveLength(
    1,
  );
  await act(async () => button("Close").click());
  expect(window.location.search).not.toContain("connect_invite");
  mocks.request.mockResolvedValue({ ok: true, result: status });
  await openConnections();
  expect(groupSelect()).toBeNull();
  expect(document.querySelector("h2")?.textContent).toContain("Local A");
  expect(button("groupConnections.invite")).toBeDefined();
});

it("discards a late Group response and stops accepting selection results after permission loss", async () => {
  let resolve: (value: unknown) => void = () => {};
  mocks.request.mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  await render();
  await openConnections();
  await openConnections("b");
  await act(async () =>
    resolve({ ok: true, result: { ...status, account_origin: "https://wrong.test" } }),
  );
  expect(
    [...document.querySelectorAll("a")].some((a) => a.href.startsWith("https://wrong.test")),
  ).toBe(false);
  await render(false);
  expect(document.querySelector('[role="dialog"]')).toBeNull();
});

it.each(["syncing", "unavailable", "not_linked"])(
  "distinguishes %s from a confirmed empty list",
  async (state) => {
    mocks.request.mockResolvedValue({
      ok: true,
      result: { ...status, status: state, expires_at: null, account_id: null },
    });
    await render();
    await openConnections();
    expect(document.body.textContent).not.toContain("groupConnections.empty");
    expect(Boolean(button("groupConnections.linkAccount"))).toBe(state === "not_linked");
    if (state !== "not_linked") expect(button("groupConnections.invite").disabled).toBe(true);
  },
);
it("clears sharing errors only after a successful fresh confirmation", async () => {
  mocks.request.mockResolvedValue({
    ok: true,
    result: {
      ...status,
      status: "unavailable",
      error_code: "connect_groups_unsupported",
      error_message: "Account needs an update",
    },
  });
  await render();
  await openConnections();
  expect(document.body.textContent).toContain("groupConnections.unsupported");
  expect(document.body.textContent).not.toContain("groupConnections.empty");
  mocks.request.mockResolvedValue({ ok: true, result: status });
  await act(async () => button("groupConnections.refresh").click());
  expect(document.querySelector('[role="alert"]')).toBeNull();
  expect(document.body.textContent).toContain("groupConnections.empty");
  expect(button("groupConnections.invite").disabled).toBe(false);
});
it("expires a displayed confirmation even while the next GET is waiting", async () => {
  vi.useFakeTimers();
  try {
    mocks.request
      .mockResolvedValueOnce({
        ok: true,
        result: { ...status, expires_at: new Date(Date.now() + 100).toISOString() },
      })
      .mockImplementation(() => new Promise(() => {}));
    await render();
    await openConnections();
    expect(document.body.textContent).toContain("groupConnections.empty");
    await act(async () => vi.advanceTimersByTimeAsync(101));
    expect(document.body.textContent).not.toContain("groupConnections.empty");
    expect(document.body.textContent).toContain("groupConnections.syncFailed");
    expect(button("groupConnections.invite").disabled).toBe(true);
  } finally {
    vi.useRealTimers();
  }
});

it("binds management to the menu's Group without following the selected chat", async () => {
  await render();
  await openConnections("b");
  expect(document.querySelector("h2")?.textContent).toContain("Local B");
  expect(groupSelect()).toBeNull();
  await render(true, "a");
  mocks.request.mockResolvedValueOnce({
    ok: true,
    result: { url: "https://account.test/connect/select?ticket=b" },
  });
  await act(async () => button("groupConnections.invite").click());
  const call = mocks.request.mock.calls.find(([, options]) => options.method === "POST")!;
  expect(JSON.parse(call[1].body)).toEqual({ group_id: "b", invitation: "" });
});

it("retains an invitation until administrator access and the Group list are available", async () => {
  const invitation = "11111111-1111-4111-8111-111111111111";
  window.history.replaceState(null, "", `/?connect_invite=${invitation}`);
  await render(false, "");
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(mocks.request).not.toHaveBeenCalled();
  await render(true, "");
  expect(groupSelect().dataset.value).toBe("a");
  expect(button("groupConnections.accept")).toBeDefined();
});

it("reuses the Group panel in settings without a picker and aborts when leaving", async () => {
  await act(async () =>
    root.render(<GroupConnectionsPanel groupId="b" onOpenAccount={() => {}} />),
  );
  expect(groupSelect()).toBeNull();
  expect(document.querySelector('[role="dialog"]')).toBeNull();
  expect(mocks.request.mock.calls[0][0]).toBe("/api/v1/connect/groups?group_id=b");
  const signal = mocks.request.mock.calls[0][1].signal as AbortSignal;
  await act(async () => root.render(null));
  expect(signal.aborted).toBe(true);
});

it("keeps Refresh available after an initial status request fails", async () => {
  mocks.request.mockResolvedValueOnce({
    ok: false,
    error: { code: "unavailable", message: "Try again" },
  });
  await render();
  await openConnections();
  expect(document.querySelector('[role="alert"]')?.textContent).toContain("Try again");
  expect(button("groupConnections.refresh")).toBeDefined();
  await act(async () => button("groupConnections.refresh").click());
  expect(mocks.request).toHaveBeenCalledTimes(2);
  expect(button("groupConnections.invite").disabled).toBe(false);
});
