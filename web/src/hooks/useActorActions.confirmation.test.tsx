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
    // The terminal's own notice says what to do; no pop-up repeats it.
    expect(useUIStore.getState().activeTab).toBe(actor.id);
    expect(useUIStore.getState().notice).toBeNull();
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
  expect(useUIStore.getState().notice).toBeNull();
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

it("offers the terminal when the user moved to another tab, and withdraws it once answered", async () => {
  vi.mocked(api.fetchActors).mockResolvedValue({
    ok: true,
    result: { actors: [{ ...actor, running: true, runtime_confirmation_required: true }] },
  });
  vi.mocked(api.startActor).mockImplementation(async () => {
    useUIStore.getState().setActiveTab("settings");
    return { ok: true, result: { runtime_confirmation_required: true } };
  });
  await act(async () => actions.toggleActorEnabled(actor));
  expect(useUIStore.getState().activeTab).toBe("settings");
  const notice = useUIStore.getState().notice!;
  expect(notice.message).toBe("Claude is waiting for your confirmation in its terminal.");
  expect(notice.actionLabel).toBe("Open confirmation terminal");
  // As the pop-up does: run the action, then close.
  await act(async () => notice.onAction!());
  useUIStore.getState().dismissNotice();
  expect(useUIStore.getState().activeTab).toBe(actor.id);

  // Answered in the terminal, the Actor no longer waits and the offer goes with it.
  useUIStore.getState().setActiveTab("chat");
  await act(async () => actions.toggleActorEnabled(actor));
  const again = useUIStore.getState().notice!;
  expect(again).not.toBe(notice);
  expect(again.onAction).toBeTypeOf("function");
  vi.mocked(api.fetchActors).mockResolvedValue({
    ok: true,
    result: { actors: [{ ...actor, running: true, runtime_confirmation_required: false }] },
  });
  await act(async () => useGroupStore.getState().refreshActors("g"));
  expect(useUIStore.getState().notice).toBeNull();
});

it("only names the Group and Actor when the user has moved to another Group", async () => {
  vi.mocked(api.startActor).mockImplementation(async () => {
    useGroupStore.getState().setSelectedGroupId("other");
    return { ok: true, result: { runtime_confirmation_required: true } };
  });
  await act(async () => actions.toggleActorEnabled(actor));
  expect(useGroupStore.getState().selectedGroupId).toBe("other");
  expect(useUIStore.getState().activeTab).toBe("chat");
  const notice = useUIStore.getState().notice!;
  expect(notice.message).toBe("g / claude is waiting for your confirmation in its terminal.");
  expect(notice.onAction).toBeUndefined();
});

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
    expect(useUIStore.getState().activeTab).toBe(actor.id);
    expect(useGroupStore.getState().actors[0].runtime_confirmation_required).toBe(true);
    expect(useUIStore.getState().actorBusy).toEqual({});
  },
);
