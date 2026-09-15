// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { SettingsModal } from "./SettingsModal";
import { writeSettingsLastLocation } from "./modals/settings/settingsLastLocation";

const mocks = vi.hoisted(() => ({ request: vi.fn(), allowed: true }));
vi.mock("../services/api/base", () => ({ apiJson: mocks.request }));
vi.mock("../services/api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../services/api")>()),
  fetchActors: vi.fn(async () => ({ ok: true, result: { actors: [] } })),
  fetchObservability: vi.fn(async () => ({
    ok: false,
    error: { code: "fixture", message: "Unavailable" },
  })),
  fetchWebAccessSession: vi.fn(async () => ({
    ok: true,
    result: {
      web_access_session: { login_active: true, can_access_global_settings: mocks.allowed },
    },
  })),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) => key,
    i18n: { language: "en", resolvedLanguage: "en" },
  }),
}));
vi.mock("./modals/settings/AccountTab", () => ({
  AccountTab: ({
    isActive,
    returnToWebAccess,
  }: {
    isActive: boolean;
    returnToWebAccess: boolean;
  }) => <div data-account-active={isActive} data-return-web-access={returnToWebAccess} />,
}));
let host: HTMLDivElement, root: ReturnType<typeof createRoot>;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  mocks.allowed = true;
  writeSettingsLastLocation({ scope: "group", groupTab: "connections", globalTab: "account" });
  mocks.request
    .mockReset()
    .mockResolvedValue({
      ok: false,
      error: { code: "fixture", message: "Unavailable in fixture" },
    });
  mocks.request.mockImplementation(async (url: string) =>
    url.startsWith("/api/v1/connect/groups")
      ? { ok: true, result: { status: "not_linked", links: [] } }
      : { ok: false, error: { code: "fixture", message: "Unavailable in fixture" } },
  );
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  localStorage.clear();
});
const render = (groupId = "a") =>
  act(async () =>
    root.render(
      <SettingsModal
        isOpen
        onClose={() => {}}
        settings={null}
        onUpdateSettings={async () => {}}
        busy={false}
        isDark={false}
        groupId={groupId}
      />,
    ),
  );
it("uses the current Group without a picker and removes the panel on permission loss", async () => {
  await render();
  expect(
    mocks.request.mock.calls.some(([url]) => url === "/api/v1/connect/groups?group_id=a"),
  ).toBe(true);
  expect(host.querySelector("[data-connect-group-select]")).toBeNull();
  await render("b");
  expect(
    mocks.request.mock.calls.some(([url]) => url === "/api/v1/connect/groups?group_id=b"),
  ).toBe(true);
  expect(host.querySelector("h3")?.textContent).toContain("b");
  const last = [...mocks.request.mock.calls]
    .reverse()
    .find(([url]) => url === "/api/v1/connect/groups?group_id=b")!;
  mocks.allowed = false;
  await render("restricted");
  expect(last[1].signal.aborted).toBe(true);
  expect(host.textContent).not.toContain("groupConnections.linkAccount");
});
it("leaves Group scope when an unlinked instance needs account setup", async () => {
  await render();
  const link = [...host.querySelectorAll("button")].find(
    (b) => b.textContent === "groupConnections.linkAccount",
  )!;
  await act(async () => link.click());
  expect(host.querySelector('[data-account-active="true"]')).not.toBeNull();
  expect(host.querySelector('[data-return-web-access="true"]')).toBeNull();
  expect(host.querySelector("[data-connect-group-select]")).toBeNull();
});

it("does not expose or poll Group connections in a restricted session", async () => {
  mocks.allowed = false;
  await render();
  expect(
    mocks.request.mock.calls.some(([url]) => String(url).startsWith("/api/v1/connect/groups")),
  ).toBe(false);
  expect(host.textContent).not.toContain("layout:groupConnections.title");
});
