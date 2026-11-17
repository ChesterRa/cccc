// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { UpdateTab } from "./UpdateTab";
import * as api from "../../../services/api";
import type { KeepRefusal, UpdateCheck, UpdateStatus } from "../../../services/api";

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
  local: null,
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
    result: { check: newer, in_place: true, local_restart: false },
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
      local_restart: false,
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
    result: { check: newer, in_place: true, local_restart: false },
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
  expect(api.startUpdate).toHaveBeenCalledWith({ source: "release", version: "0.4.59" }, "restart");
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
    result: { check: newer, in_place: true, local_restart: false },
  });
  await render();
  await click("update.check");
  await click("update.updateAndRestart");
  await click("update.updateNow");
}

it("offers keeping agents through a release update when the host can, checked once downloaded", async () => {
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: { check: newer, in_place: true, local_restart: false, release_keep: true },
  });
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: true,
    result: { accepted: true, transaction: "txn", mode: "keep" },
  });
  await render();
  await click("update.check");
  expect(button("update.updateAndRestart")).toBeUndefined();
  await click("update.updateNow");
  expect(text()).toContain('update.confirmKeep {"version":"0.4.59"}');
  expect(button("update.updateNow")).toBeUndefined();
  await click("update.keepAgents");
  expect(api.startUpdate).toHaveBeenCalledWith({ source: "release", version: "0.4.59" }, "keep");
});

it("says when agents meant to keep running were not checked after the update stopped CCCC", async () => {
  const kept = (over: Record<string, unknown>) =>
    status({
      last: installed({
        mode: "keep",
        agents: {
          state: "not_observed",
          reattached: [],
          restarted: [],
          not_back: [],
          unconfirmed: [],
        },
        ...over,
      }),
    });
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    kept({ result: "abandoned", stage: "stop", reason: "fixture" }),
  );
  await render();
  expect(text()).toContain("update.agents.notObserved");

  // Nothing stopped, so there is nothing to check.
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    kept({ result: "not_started", stage: "keep", reason: "fixture" }),
  );
  root.unmount();
  root = createRoot(host);
  await render();
  expect(text()).not.toContain("update.agents.notObserved");
});

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
    result: { check: pip, in_place: false, local_restart: false },
  });
  await render();
  await click("update.check");
  expect(text()).toContain("update.byPip");
  expect(host.querySelector("code")?.textContent).toBe(pip.command);
  expect(button("update.updateAndRestart")).toBeUndefined();
});

it("shows a rebuilt local program beside any release answer, and an unreadable one as such", async () => {
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
        local: {
          state: "rebuilt",
          version: "0.4.58",
          source_id: "a".repeat(64),
          built_at: "2026-11-17T09:40:00Z",
          token: "fixture-token",
          keep: { allowed: true, refusals: [] },
        },
      },
      in_place: false,
      local_restart: false,
    },
  });
  await render();
  await click("update.check");
  // A failed release check does not hide the rebuilt program.
  expect(text()).toContain('update.checkFailed {"reason":"timed out"}');
  expect(text()).toContain("update.localBuild");
  expect(text()).toContain("update.builtAt");
  expect(text()).not.toContain("update.localUnreadable");

  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: {
      check: { ...newer, local: { state: "unreadable", reason: "cannot check itself" } },
      in_place: true,
      local_restart: false,
    },
  });
  await click("update.check");
  expect(text()).toContain('update.localUnreadable {"reason":"cannot check itself"}');
  expect(text()).not.toContain("update.builtAt");

  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: { check: newer, in_place: true, local_restart: false },
  });
  await click("update.check");
  expect(text()).not.toContain("update.localBuild");
});

const rebuilt = (keep: { allowed: boolean; refusals: KeepRefusal[] }): UpdateCheck => ({
  ...newer,
  installation: "unmanaged",
  command: null,
  local: {
    state: "rebuilt",
    version: "0.4.58",
    source_id: "b".repeat(64),
    built_at: "2026-11-17T09:40:00Z",
    token: "fixture-token",
    keep,
  },
});

it("restarts into a rebuilt program after one choice, keeping agents when allowed", async () => {
  vi.useFakeTimers();
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: {
      // A standalone installation that also has a newer release: both are offered.
      check: { ...rebuilt({ allowed: true, refusals: [] }), installation: "standalone" },
      in_place: true,
      local_restart: true,
    },
  });
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: true,
    result: { accepted: true, transaction: "local-txn", mode: "keep" },
  });
  await render();
  await click("update.check");
  await click("update.restartToBuild");
  expect(api.startUpdate).not.toHaveBeenCalled();
  expect(text()).toContain("update.confirmLocal");
  // While this choice is open, the release's own update waits.
  expect(button("update.updateAndRestart")?.disabled).toBe(true);
  expect(button("update.keepAgents")?.disabled).toBe(false);
  await click("update.keepAgents");
  expect(api.startUpdate).toHaveBeenCalledWith(
    { source: "local", version: "0.4.58", token: "fixture-token", source_id: "b".repeat(64) },
    "keep",
  );
  expect(text()).toContain('update.restartingLocal {"version":"0.4.58"}');

  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    status({
      last: {
        transaction: "local-txn",
        from_version: "0.4.58",
        to_version: "0.4.58",
        mode: "keep",
        result: "restarted",
        stage: null,
        reason: null,
        finished_at: "2026-11-17T09:45:00Z",
      },
    }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).not.toContain("update.restartingLocal");
  expect(text()).toContain('update.last.restarted {"from":"0.4.58","to":"0.4.58","reason":"—"}');
});

it("greys out keeping agents with the reasons, and restarts them on request", async () => {
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: {
      check: rebuilt({
        allowed: false,
        refusals: [
          {
            code: "fixture_unknown_code",
            group_id: "g_1",
            actor_id: "peer",
            owner_id: null,
            display_name: "Peer",
            message: "CCCC cannot confirm Peer's tools.",
          },
          {
            code: "mcp_not_observed",
            group_id: "g_1",
            actor_id: "lead",
            owner_id: null,
            display_name: "Lead",
            message: "fixture sentence the page does not show",
          },
          {
            code: "owner_unreachable",
            group_id: null,
            actor_id: null,
            owner_id: "fixture-owner",
            display_name: null,
            message: "fixture sentence the page does not show",
          },
          {
            code: "runtime_not_covered",
            group_id: "g_1",
            actor_id: "reviewer",
            owner_id: null,
            display_name: null,
            message: "fixture sentence the page does not show",
          },
        ],
      }),
      in_place: false,
      local_restart: true,
    },
  });
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: false,
    error: { code: "local_not_supported", message: "not yet" },
  });
  await render();
  await click("update.check");
  await click("update.restartToBuild");
  expect(button("update.keepAgents")?.disabled).toBe(true);
  expect(text()).toContain("update.keepUnavailable");
  // A code this page does not know shows CCCC's own sentence, which names whom it is about.
  expect(text()).toContain("CCCC cannot confirm Peer's tools.");
  // A known one is worded here, led by the member's name when it names one.
  expect(text()).toContain(
    'update.keepReasonFor {"name":"Lead","reason":"update.keepReason.mcp_not_observed"}',
  );
  expect(text()).toContain("update.keepReason.owner_unreachable");
  // Without a display name, the member is named by its ID.
  expect(text()).toContain(
    'update.keepReasonFor {"name":"reviewer","reason":"update.keepReason.runtime_not_covered"}',
  );
  expect(text()).not.toContain('"reason":"update.keepReason.owner_unreachable"');
  expect(text()).not.toContain("fixture sentence the page does not show");
  await click("update.restartAgents");
  expect(api.startUpdate).toHaveBeenCalledWith(
    { source: "local", version: "0.4.58", token: "fixture-token", source_id: "b".repeat(64) },
    "restart",
  );
  expect(text()).toContain('update.rejectedLocal {"reason":"not yet"}');
});

it("a lost answer to a restart into a rebuilt program waits for that file's result, not another of the same version", async () => {
  vi.useFakeTimers();
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: {
      check: rebuilt({ allowed: true, refusals: [] }),
      in_place: false,
      local_restart: true,
    },
  });
  vi.mocked(api.startUpdate).mockResolvedValue({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "Failed to fetch" },
  });
  await render();
  await click("update.check");
  await click("update.restartToBuild");
  await click("update.restartAgents");
  expect(text()).toContain('update.lostLocal {"version":"0.4.58"}');

  // Another restart into a build of the same version is not this one's result.
  const restarted = (over: Record<string, unknown>) =>
    status({
      last: installed({
        transaction: "other",
        to_version: "0.4.58",
        result: "restarted",
        source: "local",
        token: "another-file",
        ...over,
      }),
    });
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(restarted({}));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain("update.lostLocal");
  // Nor is a result that does not name the file it restarted into.
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(restarted({ token: undefined }));
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain("update.lostLocal");

  // Nor one for the same file answering as another build.
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    restarted({ token: "fixture-token", source_id: "c".repeat(64) }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).toContain("update.lostLocal");

  const member = (actor_id: string, title: string | null) => ({
    group_id: "g_fixture",
    actor_id,
    title,
  });
  // The result comes first; its agents are still being looked for.
  const found = { transaction: "this", token: "fixture-token", source_id: "b".repeat(64) };
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    restarted({
      ...found,
      agents: {
        state: "observing",
        reattached: [],
        restarted: [],
        not_back: [],
        unconfirmed: [member("peer-1", "Reviewer")],
      },
    }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).not.toContain("update.lostLocal");
  expect(text()).toContain("update.agents.observing");
  expect(text()).not.toContain("update.agents.unconfirmed");

  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    restarted({
      ...found,
      agents: {
        state: "unconfirmed",
        reattached: [member("peer-1", "Reviewer"), member("peer-2", null)],
        restarted: [],
        not_back: [member("peer-3", "Writer")],
        unconfirmed: [member("peer-4", null)],
      },
    }),
  );
  await act(async () => vi.advanceTimersByTimeAsync(2000));
  expect(text()).not.toContain("update.lostLocal");
  expect(text()).toContain('update.last.restarted {"from":"0.4.58","to":"0.4.58","reason":"—"}');
  // The agents it kept, and those it could not find or confirm, each by title or ID.
  expect(text()).toContain(
    'update.agents.reattached {"names":"Reviewerupdate.agents.separatorpeer-2"}',
  );
  expect(text()).toContain('update.agents.notBack {"names":"Writer"}');
  expect(text()).toContain('update.agents.unconfirmed {"names":"peer-4"}');
  expect(text()).not.toContain("update.agents.restarted");
});

it("offers no restart into a rebuilt program on a host that cannot carry it out", async () => {
  vi.mocked(api.checkForUpdate).mockResolvedValue({
    ok: true,
    result: {
      check: rebuilt({ allowed: true, refusals: [] }),
      in_place: false,
      local_restart: false,
    },
  });
  await render();
  await click("update.check");
  expect(text()).toContain("update.localBuild");
  expect(button("update.restartToBuild")).toBeUndefined();
});

it("words a restart into a rebuilt program as a restart, never as an update to an earlier version", async () => {
  const local = (over: Record<string, unknown>) =>
    status({ last: installed({ to_version: "0.4.58", source: "local", token: "t", ...over }) });
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    local({ result: "not_started", stage: "arm", reason: "changed after it was checked" }),
  );
  await render();
  expect(text()).toContain(
    'update.last.notStartedLocal {"from":"0.4.58","to":"0.4.58","reason":"arm: changed after it was checked"}',
  );
  await act(async () => root.unmount());
  root = createRoot(host);
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(
    local({ result: "abandoned", stage: "recover" }),
  );
  await render();
  // Recovery changed no files and started nothing: no earlier version came back.
  expect(text()).toContain("update.last.abandonedLocal");
  expect(text()).not.toContain("update.last.abandoned ");
  await act(async () => root.unmount());
  root = createRoot(host);
  vi.mocked(api.fetchUpdateStatus).mockResolvedValue(local({ result: "unconfirmed" }));
  await render();
  expect(text()).toContain("update.last.unconfirmedLocal");
});
