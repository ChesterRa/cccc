// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

const api = vi.hoisted(() => ({
  fetchGroupSpaceStatus: vi.fn(),
  controlGroupSpaceProviderAuth: vi.fn(),
  fetchGroupSpaceSpaces: vi.fn(),
  bindGroupSpace: vi.fn(),
  unbindGroupSpace: vi.fn(),
  getGroupSpaceProviderAuthBrowserWebSocketUrl: vi.fn(() => ""),
  fetchGroupSpaceProviderAuthBrowserSession: vi.fn(),
}));
vi.mock("../../../services/api", () => api);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
// The projected login browser is its own component; this page only decides when to show it.
vi.mock("../../browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: () => null,
}));
import { GroupSpaceTab } from "./GroupSpaceTab";

const SPACES = [
  { remote_space_id: "nb-work", title: "Work notebook", created_at: "", is_owner: true },
  { remote_space_id: "nb-other", title: "Other notebook", created_at: "", is_owner: true },
];
function mockConnection(connected: boolean) {
  api.fetchGroupSpaceStatus.mockResolvedValue({
    ok: true,
    result: {
      provider: { auth_configured: connected, write_ready: connected },
      bindings: connected
        ? { work: { status: "bound", remote_space_id: "nb-work" }, memory: null }
        : {},
    },
  });
  api.controlGroupSpaceProviderAuth.mockResolvedValue({
    ok: true,
    result: { auth: { state: "idle" } },
  });
  api.fetchGroupSpaceSpaces.mockResolvedValue({ ok: true, result: { spaces: SPACES } });
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.resetAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  vi.unstubAllGlobals();
  await act(async () => root.unmount());
  host.remove();
});
const mount = () => act(async () => root.render(<GroupSpaceTab isDark={false} groupId="g1" />));
const cards = () => [...host.querySelectorAll("[data-settings-card]")];
const cardTitles = () => cards().map((card) => card.querySelector("h3")?.textContent);
const card = (title: string) =>
  cards().find((node) => node.querySelector("h3")?.textContent === title)!;
const button = (label: string, scope: ParentNode = host) =>
  [...scope.querySelectorAll<HTMLButtonElement>("button")].find((b) => b.textContent === label);
const notebookSelectors = () =>
  [...host.querySelectorAll<HTMLButtonElement>('button[role="combobox"]')].filter(
    (element) => element.getAttribute("aria-label") === "groupSpace.chooseNotebook",
  );

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function connectedStatus(bound: boolean) {
  return {
    ok: true,
    result: {
      provider: { auth_configured: true, write_ready: true },
      bindings: {
        work: bound ? { status: "bound", remote_space_id: "nb-work" } : null,
        memory: null,
      },
    },
  };
}

it("starts a Google connection from the account card and locks notebooks until then", async () => {
  mockConnection(false);
  await mount();
  expect(cardTitles()).toEqual([
    "groupSpace.accountTitle",
    "groupSpace.notebookSectionLockedTitle",
  ]);
  const account = card("groupSpace.accountTitle");
  const statusRow = account.querySelector("[data-settings-row]")!;
  expect(statusRow.textContent).toContain("common:status");
  expect(statusRow.textContent).toContain("groupSpace.accountNotConnected");
  expect(button("groupSpace.disconnectGoogle")).toBeUndefined();

  await act(async () => button("groupSpace.connectGoogle", statusRow)!.click());
  expect(api.controlGroupSpaceProviderAuth).toHaveBeenCalledWith(
    expect.objectContaining({ action: "start", forceReauth: false }),
  );
});

it.each(["stay", "group", "inactive"])(
  "finishes successful auth feedback using page ownership (after=%s)",
  async (after) => {
    mockConnection(false);
    const polled = deferred<{
      ok: true;
      result: { auth: { state: string; session_id: string } };
    }>();
    const refreshed = deferred<ReturnType<typeof connectedStatus>>();
    let started = false;
    api.controlGroupSpaceProviderAuth.mockImplementation(async ({ action }: { action: string }) => {
      if (action === "start") {
        started = true;
        return { ok: true, result: { auth: { state: "running" } } };
      }
      return started ? polled.promise : { ok: true, result: { auth: { state: "idle" } } };
    });
    await mount();
    api.fetchGroupSpaceStatus
      .mockReturnValueOnce(refreshed.promise)
      .mockResolvedValue(connectedStatus(true));
    await act(async () => button("groupSpace.connectGoogle")!.click());
    await act(async () =>
      polled.resolve({
        ok: true,
        result: { auth: { state: "succeeded", session_id: "synthetic-flow" } },
      }),
    );
    expect(api.fetchGroupSpaceStatus).toHaveBeenCalledTimes(2);
    if (after !== "stay") {
      await act(async () =>
        root.render(
          <GroupSpaceTab
            isDark={false}
            groupId={after === "group" ? "g2" : "g1"}
            isActive={after !== "inactive"}
          />,
        ),
      );
    }
    await act(async () => refreshed.resolve(connectedStatus(true)));
    if (after === "stay") expect(host.textContent).toContain("groupSpace.googleConnectSuccess");
    else expect(host.textContent).not.toContain("groupSpace.googleConnectSuccess");
  },
);

it("releases a departed auth action's busy state when the same page is reopened", async () => {
  mockConnection(false);
  const started = deferred<{ ok: true; result: { auth: { state: string } } }>();
  api.controlGroupSpaceProviderAuth.mockImplementation(async ({ action }: { action: string }) =>
    action === "start" ? started.promise : { ok: true, result: { auth: { state: "idle" } } },
  );
  await mount();
  await act(async () => button("groupSpace.connectGoogle")!.click());
  expect(button("groupSpace.connectGoogle")!.disabled).toBe(true);
  await act(async () =>
    root.render(<GroupSpaceTab isDark={false} groupId="g1" isActive={false} />),
  );
  await mount();
  await act(async () => started.resolve({ ok: true, result: { auth: { state: "succeeded" } } }));
  expect(button("groupSpace.connectGoogle")!.disabled).toBe(false);
  expect(host.textContent).not.toContain("groupSpace.connectStarted");
});

it("binds a chosen notebook per lane and asks before destructive actions", async () => {
  mockConnection(true);
  await mount();
  expect(cardTitles()).toEqual([
    "groupSpace.accountTitle",
    "groupSpace.workNotebookTitle",
    "groupSpace.memoryNotebookTitle",
  ]);

  const work = card("groupSpace.workNotebookTitle");
  expect(work.textContent).toContain("Work notebook");
  const unbind = button("groupSpace.unbind", work)!;
  // The unbind action is lane specific, so it stays in its own card.
  expect(button("groupSpace.unbind", card("groupSpace.memoryNotebookTitle"))).toBeUndefined();

  const confirm = vi.fn(() => false);
  vi.stubGlobal("confirm", confirm);
  await act(async () => unbind.click());
  expect(confirm).toHaveBeenCalledWith("groupSpace.workUnbindConfirm");
  expect(api.unbindGroupSpace).not.toHaveBeenCalled();
  await act(async () => button("groupSpace.disconnectGoogle")!.click());
  expect(confirm).toHaveBeenCalledWith("groupSpace.disconnectConfirm");
  expect(api.controlGroupSpaceProviderAuth).not.toHaveBeenCalledWith(
    expect.objectContaining({ action: "disconnect" }),
  );

  api.bindGroupSpace.mockResolvedValue({ ok: false, error: { message: "bind failed" } });
  const memory = card("groupSpace.memoryNotebookTitle");
  await act(async () => button("groupSpace.bindSelected", memory)!.click());
  expect(api.bindGroupSpace).toHaveBeenCalledWith("g1", "nb-work", "notebooklm", "memory");
  expect(host.querySelector('[role="alert"]')?.textContent).toBe("bind failed");
});

it.each(["group", "reconnect"] as const)(
  "releases selectors after an unbind supersedes the %s notebook read with a cached refresh",
  async (trigger) => {
    mockConnection(true);
    vi.stubGlobal("confirm", () => true);
    let bound = true;
    let reconnecting = false;
    const listRead = deferred<{ ok: boolean; result: { spaces: typeof SPACES } }>();
    api.fetchGroupSpaceStatus.mockImplementation(async () => connectedStatus(bound));
    api.controlGroupSpaceProviderAuth.mockImplementation(async ({ action }: { action: string }) => {
      if (action === "start") {
        reconnecting = true;
        return { ok: true, result: { auth: { state: "running" } } };
      }
      return { ok: true, result: { auth: { state: reconnecting ? "succeeded" : "idle" } } };
    });
    api.fetchGroupSpaceSpaces
      .mockResolvedValueOnce({ ok: true, result: { spaces: SPACES } })
      .mockReturnValueOnce(listRead.promise);
    api.unbindGroupSpace.mockImplementation(async () => {
      bound = false;
      return connectedStatus(false);
    });

    await mount();
    expect(notebookSelectors()).toHaveLength(2);
    expect(notebookSelectors().every((element) => !element.disabled)).toBe(true);
    await act(async () => {
      if (trigger === "group") root.render(<GroupSpaceTab isDark={false} groupId="g2" />);
      else button("groupSpace.switchGoogle")!.click();
    });
    expect(api.fetchGroupSpaceSpaces).toHaveBeenCalledTimes(2);
    expect(notebookSelectors().every((element) => element.disabled)).toBe(true);

    await act(async () => button("groupSpace.unbind")!.click());
    expect(api.unbindGroupSpace).toHaveBeenCalledWith(
      trigger === "group" ? "g2" : "g1",
      "notebooklm",
      "work",
    );
    expect(notebookSelectors().every((element) => !element.disabled)).toBe(true);

    await act(async () => {
      listRead.resolve({ ok: true, result: { spaces: [] } });
      await listRead.promise;
    });
    await act(async () => button("groupSpace.refresh")!.click());
    expect(api.fetchGroupSpaceSpaces).toHaveBeenCalledTimes(2);
    expect(notebookSelectors().every((element) => !element.disabled)).toBe(true);
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(host.textContent).toContain("Work notebook");
  },
);

it("keeps selectors busy until the current Group read finishes even if an older read fails", async () => {
  mockConnection(true);
  const oldRead = deferred<{ ok: boolean; result: { spaces: typeof SPACES } }>();
  const currentRead = deferred<{ ok: boolean; result: { spaces: typeof SPACES } }>();
  api.fetchGroupSpaceSpaces
    .mockResolvedValueOnce({ ok: true, result: { spaces: SPACES } })
    .mockReturnValueOnce(oldRead.promise)
    .mockReturnValueOnce(currentRead.promise);
  await mount();
  await act(async () => root.render(<GroupSpaceTab isDark={false} groupId="g2" />));
  await act(async () => root.render(<GroupSpaceTab isDark={false} groupId="g3" />));
  expect(api.fetchGroupSpaceSpaces).toHaveBeenCalledTimes(3);
  await act(async () => {
    oldRead.reject(new Error("Old Group list failed"));
    await oldRead.promise.catch(() => {});
  });
  expect(host.querySelector('[role="alert"]')).toBeNull();
  expect(notebookSelectors().every((element) => element.disabled)).toBe(true);
  await act(async () => {
    currentRead.resolve({ ok: true, result: { spaces: SPACES } });
    await currentRead.promise;
  });
  expect(notebookSelectors().every((element) => !element.disabled)).toBe(true);
});
