// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createInstance } from "i18next";
import { I18nextProvider, initReactI18next } from "react-i18next";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import enLayout from "../../../i18n/locales/en/layout.json";
import enCommon from "../../../i18n/locales/en/common.json";
import type { LocalSessionSource } from "../../../types";
import type { LocalSessionDirectory } from "../../../services/api/localSessionControls";
import {
  deferDirectoryRead,
  sourceControlFixture,
} from "../../../features/localSessions/LocalSourceControl.fixtures";
import { SessionAccessTab } from "./SessionAccessTab";

const mocks = vi.hoisted(() => ({ fetch: vi.fn(), deviceSources: vi.fn(), nodes: vi.fn() }));
vi.mock("../../../services/api/localSessions", async (original) => ({
  ...(await original<typeof import("../../../services/api/localSessions")>()),
  fetchLocalSessions: mocks.fetch,
}));
vi.mock("../../../services/api/sessionNodes", async (original) => ({
  ...(await original<typeof import("../../../services/api/sessionNodes")>()),
  fetchDeviceSessionSources: mocks.deviceSources,
  fetchSessionNodes: mocks.nodes,
}));

const labels = enLayout.sourceControls;
const source: LocalSessionSource = {
  source_id: "local:codex:fixture",
  runtime: "codex",
  cwd: "/fixture/project",
  title: "Fixture workspace",
  connected: false,
};
const i18n = createInstance();
let host: HTMLDivElement;
let root: Root;

const button = (label: string) => {
  const found = [...document.querySelectorAll("button")].find(
    (el) => el.textContent === label || el.getAttribute("aria-label") === label,
  );
  expect(found, label).toBeDefined();
  return found!;
};
const click = async (element: Element | null | undefined) => {
  expect(element).toBeTruthy();
  await act(async () => (element as HTMLElement).click());
};
const tick = (ms: number) => act(async () => vi.advanceTimersByTimeAsync(ms));
const mount = (active: boolean) =>
  act(async () =>
    root.render(
      <I18nextProvider i18n={i18n}>
        <SessionAccessTab isActive={active} />
      </I18nextProvider>,
    ),
  );

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  mocks.fetch.mockReset();
  mocks.deviceSources.mockReset();
  mocks.nodes.mockReset();
  mocks.nodes.mockResolvedValue({
    ok: true,
    result: { supported: true, devices: [], supported_runtimes: ["codex"] },
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
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it("manages this computer's sources from settings: recheck, remove with confirmation, finish a pause and restore", async () => {
  const publish = (directory: LocalSessionDirectory) =>
    mocks.fetch.mockResolvedValue({ ok: true, result: directory });
  const fixture = sourceControlFixture(source, publish);
  // A completed change also refreshes the Group list; answer it so only source routes reach the fixture.
  const groupReads = vi.fn(async () => Response.json({ ok: true, result: { groups: [] } }));
  vi.stubGlobal("fetch", (url: unknown, init?: RequestInit) =>
    String(url) === "/api/v1/groups" ? groupReads() : fixture.request(url, init),
  );
  fixture.refresh();
  await mount(true);
  await tick(0);
  const sourceRow = () =>
    [...host.querySelectorAll("li")].find(
      (li) => !li.closest("details") && li.textContent?.includes(source.title),
    );
  expect(sourceRow()).toBeDefined();
  // A source that was never checked shows no receive status until a check answers.
  expect(document.body.textContent).not.toContain(labels.reasons.unknown);
  expect(fixture.request).not.toHaveBeenCalled();
  await click(button(labels.recheck));
  expect(document.body.textContent).toContain(labels.reasons.thread_not_loaded);
  expect(document.body.textContent).toContain(labels.checkCached);
  expect(button(labels.checkLater).disabled).toBe(true);
  await tick(3000);
  expect(fixture.request).toHaveBeenCalledTimes(1);

  // Cancelling the confirmation writes nothing and returns focus to the row's action.
  const removeButton = button(labels.remove);
  await click(removeButton);
  expect(document.body.textContent).toContain(labels.removeConfirm);
  await click(button(enCommon.cancel));
  await tick(0);
  expect(document.activeElement).toBe(removeButton);
  expect(fixture.request).toHaveBeenCalledTimes(1);

  // An in-flight removal ignores repeat clicks; an unconfirmed failure keeps the source listed.
  fixture.ready();
  await tick(3000);
  await click(button(labels.remove));
  await click(button(labels.confirmRemove));
  expect(button(labels.working).disabled).toBe(true);
  const pendingCalls = fixture.request.mock.calls.length;
  await click(button(labels.working));
  expect(fixture.request).toHaveBeenCalledTimes(pendingCalls);
  await act(async () => fixture.failPending());
  expect(document.body.textContent).toContain(labels.unconfirmed);
  expect(sourceRow()).toBeDefined();

  // A stale read that lands after a partial removal must not bring the source back.
  const staleDirectory = fixture.snapshot();
  const finishStaleRead = deferDirectoryRead(mocks.fetch);
  await tick(3000);
  fixture.setMode("partial-error");
  await click(button(labels.confirmRemove));
  await act(async () => finishStaleRead(staleDirectory));
  expect(document.body.textContent).toContain(labels.partial);
  expect(document.body.textContent).toContain(labels.unconfirmed);
  expect(sourceRow()).toBeUndefined();
  await click(button(enCommon.cancel));
  await tick(0);
  // The removed row is gone, so focus returns to the section heading instead of the page body.
  expect(document.activeElement).toBe(host.querySelector("[data-source-control-return]"));

  const removed = [...host.querySelectorAll("details")].find((x) =>
    x.querySelector("summary")?.textContent?.includes(labels.removedList),
  )!;
  await click(removed.querySelector("summary"));
  expect(button(labels.restore).disabled).toBe(true);
  fixture.setMode("complete");
  await click(button(labels.finishPause));
  const confirmation = [...document.querySelectorAll('[role="dialog"]')].at(-1)!;
  await click(
    [...confirmation.querySelectorAll("button")].find((x) => x.textContent === labels.finishPause),
  );
  expect(document.body.textContent).toContain(labels.removedNotice);
  await tick(3000);
  expect(button(labels.restore).disabled).toBe(false);

  fixture.setMode("restore-conflict");
  await click(button(labels.restore));
  await click(button(labels.confirmRestore));
  expect(document.body.textContent).toContain(labels.conflict);
  fixture.setMode("complete");
  await click(button(labels.confirmRestore));
  expect(document.body.textContent).toContain(labels.restoredNotice);
  await tick(3000);
  expect(sourceRow()).toBeDefined();
  const bodies = fixture.request.mock.calls.map(([, init]) => JSON.parse(String(init?.body)));
  expect(bodies.at(-1)).toEqual({
    source_id: source.source_id,
    action: "restore",
    expected_revision: "management-conflict",
  });
  // Settings reads only this computer's directory; other computers come from the paired list.
  expect(mocks.deviceSources).not.toHaveBeenCalled();
  expect(fixture.request.mock.calls.every(([url]) => !String(url).includes("session_nodes"))).toBe(
    true,
  );
});

it("marks only a session that is not connected", async () => {
  mocks.fetch.mockResolvedValue({
    ok: true,
    result: { sources: [{ ...source, connected: true }] },
  });
  await mount(true);
  await tick(0);
  expect(host.textContent).toContain(source.title);
  expect(host.textContent).not.toContain(enLayout.localSessions.unavailable);
  expect(host.querySelector("[data-status-dot]")).toBeNull();
});

it("stops reading while the tab is hidden and reads again when shown", async () => {
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [source] } });
  await mount(true);
  await tick(0);
  expect(host.textContent).toContain(source.title);
  expect(host.textContent).toContain(enLayout.sessionAccess.otherComputers);
  expect(host.textContent).toContain(enLayout.localSessions.unavailable);
  const shownReads = mocks.fetch.mock.calls.length;
  const shownNodeReads = mocks.nodes.mock.calls.length;
  expect(shownReads).toBeGreaterThan(0);
  expect(shownNodeReads).toBeGreaterThan(0);
  await mount(false);
  await tick(15000);
  expect(mocks.fetch.mock.calls.length).toBe(shownReads);
  expect(mocks.nodes.mock.calls.length).toBe(shownNodeReads);
  await mount(true);
  await tick(0);
  expect(mocks.fetch.mock.calls.length).toBeGreaterThan(shownReads);
  expect(mocks.nodes.mock.calls.length).toBeGreaterThan(shownNodeReads);
});

it("explains settings-scoped setup refresh and lists troubleshooting for all supported runtimes", async () => {
  mocks.fetch.mockResolvedValue({ ok: true, result: { sources: [] } });
  await mount(true);
  await tick(0);
  const setup = [...host.querySelectorAll("details")].find(
    (section) => section.querySelector("summary")?.textContent === enLayout.localSessions.setup,
  )!;
  await click(setup.querySelector("summary"));
  expect(setup.textContent).toContain(enLayout.localSessions.setupFinishInSettings);
  const readsBefore = mocks.fetch.mock.calls.length;
  await click(button(enLayout.localSessions.refreshSessions));
  expect(mocks.fetch.mock.calls.length).toBeGreaterThan(readsBefore);

  const troubleshooting = [...host.querySelectorAll("details")].find(
    (section) =>
      section.querySelector("summary")?.textContent === enLayout.localSessions.troubleshooting,
  )!;
  expect(troubleshooting.textContent).toContain(enLayout.localSessions.troubleshootCodex);
  expect(troubleshooting.textContent).toContain(enLayout.localSessions.troubleshootClaude);
  expect(troubleshooting.textContent).toContain(enLayout.localSessions.troubleshootPi);
  expect(troubleshooting.textContent).toContain(enLayout.localSessions.troubleshootOpencode);
});
