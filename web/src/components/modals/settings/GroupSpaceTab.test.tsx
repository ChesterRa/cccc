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
  vi.clearAllMocks();
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
