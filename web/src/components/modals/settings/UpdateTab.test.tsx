// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { UpdateTab } from "./UpdateTab";
import * as api from "../../../services/api";
import type { UpdateCheck, UpdateStatus } from "../../../services/api";

vi.mock("../../../services/api", () => ({
  fetchUpdateStatus: vi.fn(),
  checkForUpdate: vi.fn(),
  startUpdate: vi.fn(),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: Record<string, unknown>) =>
      values ? `${key} ${JSON.stringify(values)}` : key,
    i18n: { language: "en" },
  }),
}));

const status = (over: Partial<UpdateStatus> = {}) => ({
  ok: true as const,
  result: {
    status: { current: "0.4.58", installation: "standalone", last: null, ...over },
    in_place: true,
  },
});
const newer: UpdateCheck = {
  current: "0.4.58",
  channel: "stable",
  installation: "standalone",
  latest: "0.4.59",
  status: "newer",
  failure: null,
  release_notes_url: "https://github.com/ChesterRa/cccc/releases/tag/v0.4.59",
  command: "cccc update --channel stable",
};

let root: ReturnType<typeof createRoot>, host: HTMLDivElement;
beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.clearAllMocks();
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(status());
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

const render = () => act(async () => root.render(<UpdateTab />));
const text = () => host.textContent || "";
const button = (label: string) =>
  [...host.querySelectorAll("button")].find((element) => element.textContent === label);
const click = (label: string) =>
  act(async () => {
    const target = button(label);
    if (!target) throw new Error(`no button ${label}`);
    target.click();
  });

it("opens with local facts only and asks release discovery once per click", async () => {
  await render();
  expect(api.fetchUpdateStatus).toHaveBeenCalledTimes(1);
  expect(api.checkForUpdate).not.toHaveBeenCalled();
  expect(text()).toContain("0.4.58");

  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: { check: newer, in_place: true },
  });
  await click("update.check");
  expect(api.checkForUpdate).toHaveBeenCalledTimes(1);
  expect(text()).toContain("0.4.59");
  expect(host.querySelector("a")?.getAttribute("href")).toBe(newer.release_notes_url);
});

it("shows a failed check as failed, never as up to date", async () => {
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: {
      check: {
        ...newer,
        latest: null,
        status: null,
        release_notes_url: null,
        command: null,
        failure: "timed out",
      },
      in_place: true,
    },
  });
  await render();
  await click("update.check");
  expect(text()).toContain('update.checkFailed {"reason":"timed out"}');
  expect(text()).not.toContain("update.upToDate");
  expect(button("update.updateAndRestart")).toBeUndefined();
});

it("updates after one confirmation and shows this update's result when CCCC is back", async () => {
  vi.useFakeTimers();
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: { check: newer, in_place: true },
  });
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: true,
    result: { accepted: true, transaction: "txn", mode: "restart" },
  });
  await render();
  await click("update.check");
  await click("update.updateAndRestart");
  expect(api.startUpdate).not.toHaveBeenCalled();
  expect(text()).toContain('update.confirmText {"version":"0.4.59"}');
  await click("update.updateNow");
  expect(api.startUpdate).toHaveBeenCalledTimes(1);
  expect(api.startUpdate).toHaveBeenCalledWith("0.4.59");
  expect(text()).toContain('update.updating {"version":"0.4.59"}');

  // While CCCC is away, and after it is back with an older result, the page keeps waiting.
  vi.mocked(api.fetchUpdateStatus).mockResolvedValueOnce({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
  });
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain("update.updating");
  const result = {
    transaction: "txn",
    from_version: "0.4.58",
    to_version: "0.4.59",
    mode: "restart",
    result: "installed",
    stage: null,
    reason: null,
    finished_at: "2026-11-17T08:00:00Z",
  };
  vi.mocked(api.fetchUpdateStatus).mockResolvedValueOnce(
    status({ last: { ...result, transaction: "earlier" } }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain("update.updating");
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(status({ current: "0.4.59", last: result }));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).not.toContain("update.updating");
  expect(text()).toContain('update.last.installed {"from":"0.4.58","to":"0.4.59","reason":"—"}');
});

const installed = (over: Record<string, unknown> = {}) => ({
  transaction: "txn",
  from_version: "0.4.58",
  to_version: "0.4.59",
  mode: "restart",
  result: "installed",
  stage: null,
  reason: null,
  finished_at: "2026-11-17T08:00:00Z",
  ...over,
});

async function confirmUpdate() {
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: { check: newer, in_place: true },
  });
  await render();
  await click("update.check");
  await click("update.updateAndRestart");
  await click("update.updateNow");
}

it("a lost answer waits for a result recorded after the start, never an earlier one", async () => {
  vi.useFakeTimers();
  const earlier = installed({
    transaction: "earlier",
    from_version: "0.4.57",
    to_version: "0.4.58",
  });
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(status({ last: earlier }));
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
  });
  await confirmUpdate();
  expect(text()).toContain('update.lost {"version":"0.4.59"}');
  expect(text()).not.toContain("update.rejected");

  // The old CCCC still answers with the earlier result: that is not this update's.
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain("update.lost");

  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    status({ current: "0.4.59", last: installed({ transaction: "new" }) }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).not.toContain("update.lost");
  expect(text()).toContain('update.last.installed {"from":"0.4.58","to":"0.4.59","reason":"—"}');
});

it("only CCCC's own error reads as the update not starting", async () => {
  vi.useFakeTimers();
  // A proxy's error page and an empty answer did not come from CCCC.
  for (const answer of [
    { ok: false as const, error: { code: "HTTP_ERROR", message: "Server returned 524" } },
    { ok: true as const, result: {} as never },
  ]) {
    vi.mocked(api.startUpdate).mockResolvedValue(answer);
    await confirmUpdate();
    expect(text()).toContain("update.lost");
    expect(text()).not.toContain("update.rejected");
    await act(async () => root.unmount());
    root = createRoot(host);
  }

  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: false,
    error: { code: "update_busy", message: "another update holds this CCCC" },
  });
  await confirmUpdate();
  expect(text()).toContain('update.rejected {"reason":"another update holds this CCCC"}');
});

it("past the patience it says whether CCCC answers, and keeps waiting for the result", async () => {
  vi.useFakeTimers();
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: true,
    result: { accepted: true, transaction: "txn", mode: "restart" },
  });
  await confirmUpdate();
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
  });
  await act(async () => vi.advanceTimersByTimeAsync(5 * 60 * 1000 + 2000));
  expect(text()).toContain('update.notBack {"version":"0.4.59"}');

  // Back, but with no result for this update: say so instead of "not back".
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(status({ current: "0.4.59" }));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain('update.noResult {"version":"0.4.59"}');
  expect(button("update.check")?.disabled).toBe(false);

  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    status({ current: "0.4.59", last: installed() }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).not.toContain("update.noResult");
  expect(text()).toContain("update.last.installed");
});

it("names the command when this CCCC cannot replace itself", async () => {
  const pip = {
    ...newer,
    installation: "pip",
    command: 'python -m pip install --upgrade "cccc-pair==0.4.59"',
  };
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: { check: pip, in_place: false },
  });
  await render();
  await click("update.check");
  expect(text()).toContain("update.byPip");
  expect(host.querySelector("code")?.textContent).toBe(pip.command);
  expect(button("update.updateAndRestart")).toBeUndefined();
});
