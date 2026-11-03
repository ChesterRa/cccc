// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useGroupStore, useUIStore } from "../stores";
import { useActorActions } from "./useActorActions";
import { i18nReady } from "../i18n";
import type { Actor } from "../types";

const actor: Actor = { id: "claude", title: "Claude", runtime: "claude", running: false };
let root: Root;
let host: HTMLDivElement;
let actions: ReturnType<typeof useActorActions>;
let originalGroup: ReturnType<typeof useGroupStore.getState>;
let originalUI: ReturnType<typeof useUIStore.getState>;

beforeEach(async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  await i18nReady;
  originalGroup = useGroupStore.getState();
  originalUI = useUIStore.getState();
  useGroupStore.setState({ selectedGroupId: "g", actors: [actor], groups: [{ group_id: "g" }] });
  useUIStore.setState({ activeTab: "chat", actorBusy: {}, chatSessions: {}, notice: null });
  vi.spyOn(api, "fetchActors").mockResolvedValue({ ok: true, result: { actors: [actor] } });
  vi.spyOn(api, "fetchGroups").mockResolvedValue({
    ok: true,
    result: { groups: [{ group_id: "g" }, { group_id: "other" }] },
  });
  vi.spyOn(api, "startActor").mockResolvedValue({ ok: true, result: {} });
  vi.spyOn(api, "restartActor").mockResolvedValue({ ok: true, result: {} });
  host = document.createElement("div");
  root = createRoot(host);
  function Probe() {
    actions = useActorActions("g");
    return null;
  }
  await act(async () => root.render(<Probe />));
});

afterEach(async () => {
  await act(async () => root.unmount());
  useUIStore.getState().dismissNotice();
  useUIStore.getState().dismissError();
  useGroupStore.setState(originalGroup, true);
  useUIStore.setState(originalUI, true);
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

it.each(["start", "restart"] as const)(
  "selects the Actor terminal route when %s awaits confirmation",
  async (action) => {
    vi.mocked(action === "start" ? api.startActor : api.restartActor).mockResolvedValue({
      ok: true,
      result: { runtime_confirmation_required: true },
    });
    vi.mocked(api.fetchActors).mockResolvedValue({
      ok: true,
      result: {
        actors: [
          { ...actor, running: true, runner_effective: "pty", runtime_state_source: "terminal" },
        ],
      },
    });
    await act(async () =>
      action === "start" ? actions.toggleActorEnabled(actor) : actions.relaunchActor(actor),
    );
    expect(useUIStore.getState().activeTab).toBe(actor.id);
    expect(useUIStore.getState().notice?.message).toContain("confirmation");
    expect(useUIStore.getState().notice?.message).toContain("keep the terminal open");
    expect(useUIStore.getState().actorBusy).toEqual({});
    expect(useGroupStore.getState().actors[0].runtime_state_source).toBe("terminal");
  },
);

it("recognizes the refreshed Actor flag when the action response omits it", async () => {
  vi.mocked(api.fetchActors).mockResolvedValue({
    ok: true,
    result: { actors: [{ ...actor, running: true, runtime_confirmation_required: true }] },
  });
  await act(async () => actions.toggleActorEnabled(actor));
  expect(useUIStore.getState().activeTab).toBe(actor.id);
  expect(useUIStore.getState().notice?.message).toContain("confirmation");
});

it("keeps ordinary success and errors on their existing paths", async () => {
  await act(async () => actions.toggleActorEnabled(actor));
  expect(useUIStore.getState().activeTab).toBe("chat");
  expect(useUIStore.getState().notice).toBeNull();
  vi.mocked(api.startActor).mockResolvedValue({
    ok: false,
    error: { code: "failed", message: "Fixture launch failed" },
  });
  await act(async () => actions.toggleActorEnabled(actor));
  expect(useUIStore.getState().errorMsg).toBe("failed: Fixture launch failed");
  expect(useUIStore.getState().notice).toBeNull();
  expect(useUIStore.getState().actorBusy).toEqual({});
});

it.each([false, true])(
  "keeps late confirmation actionable and respects a later tab choice (%s)",
  async (changedTab) => {
    vi.useFakeTimers();
    vi.mocked(api.fetchActors).mockResolvedValue({
      ok: true,
      result: {
        actors: [
          { ...actor, running: true, runtime_confirmation_required: true },
          { id: "other-actor", runtime: "claude", running: true },
        ],
      },
    });
    vi.mocked(api.startActor).mockImplementation(async () => {
      useGroupStore.getState().setSelectedGroupId("other");
      return { ok: true, result: { runtime_confirmation_required: true } };
    });
    await act(async () => actions.toggleActorEnabled(actor));
    expect(useUIStore.getState().activeTab).toBe("chat");
    expect(useGroupStore.getState().selectedGroupId).toBe("other");
    const notice = useUIStore.getState().notice!;
    expect(notice.actionLabel).toContain("Open confirmation terminal");
    expect(notice.actionLabel).toContain("g / claude");
    expect(notice.onAction).toBeTypeOf("function");
    await act(async () => vi.advanceTimersByTimeAsync(4000));
    expect(useUIStore.getState().notice).toBe(notice);
    await act(async () => notice.onAction!());
    expect(useGroupStore.getState().selectedGroupId).toBe("g");
    expect(useUIStore.getState().activeTab).toBe("chat");
    if (changedTab) useUIStore.getState().setActiveTab("other-actor");
    await act(async () => useGroupStore.getState().refreshActors("g"));
    expect(useUIStore.getState().activeTab).toBe(changedTab ? "other-actor" : actor.id);
  },
);

it.each(["start", "restart"] as const)(
  "preserves a %s failure while revealing restored confirmation",
  async (action) => {
    vi.mocked(action === "start" ? api.startActor : api.restartActor).mockResolvedValue({
      ok: false,
      error: { code: "rollback_failed", message: "Fixture persistence failure" },
    });
    vi.mocked(api.fetchActors).mockResolvedValue({
      ok: true,
      result: { actors: [{ ...actor, running: true, runtime_confirmation_required: true }] },
    });
    await act(async () =>
      action === "start" ? actions.toggleActorEnabled(actor) : actions.relaunchActor(actor),
    );
    expect(useUIStore.getState().errorMsg).toBe("rollback_failed: Fixture persistence failure");
    expect(useUIStore.getState().notice?.message).toContain("confirmation");
    expect(useUIStore.getState().activeTab).toBe(actor.id);
    expect(useGroupStore.getState().actors[0].runtime_confirmation_required).toBe(true);
    expect(useUIStore.getState().actorBusy).toEqual({});
  },
);
