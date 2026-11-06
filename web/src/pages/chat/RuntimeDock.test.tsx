// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, LedgerEvent } from "../../types";
import { setUiStylePrefs } from "../../utils/uiStyle";
import { RuntimeDock } from "./RuntimeDock";
import type { LiveWorkCard } from "./liveWorkCards";
import { getRuntimeRingTone } from "./runtimeDockRingTone";

const mocks = vi.hoisted(() => ({
  events: [] as LedgerEvent[],
  workingState: "",
  open: vi.fn(),
  store: {} as object,
}));
vi.mock("../../stores", () => ({
  useGroupStore: (select: (state: object) => unknown) => select(mocks.store),
  selectChatBucketState: () => ({ events: mocks.events }),
}));
vi.mock("../../hooks/useActorDisplayState", () => ({
  useActorDisplayState: ({ actor }: { actor: Actor }) => ({
    isRunning: actor.running,
    workingState: mocks.workingState || actor.effective_working_state,
  }),
}));
vi.mock("../../components/ActorAvatar", () => ({ ActorAvatar: () => <span>avatar</span> }));
vi.mock("./RuntimeDockTicker", () => ({ RuntimeDockTicker: () => null }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) => String(options?.defaultValue || key),
  }),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let host: HTMLDivElement;
let root: Root;
const now = Date.parse("2026-09-26T10:10:00Z");
const actor: Actor = {
  id: "alpha",
  runtime: "codex",
  runner: "headless",
  runtime_state_source: "managed_session",
  enabled: true,
  running: true,
  effective_working_state: "working",
  effective_working_updated_at: "2026-09-26T10:00:00Z",
  unread_count: 2,
};
async function render(
  overrides: Partial<Actor> = {},
  groupId = "g1",
  liveWorkCards: LiveWorkCard[] = [],
) {
  await act(async () => {
    root.render(
      <RuntimeDock
        groupId={groupId}
        runtimeActors={[{ ...actor, ...overrides }]}
        liveWorkCards={liveWorkCards}
        isDark={false}
        isSmallScreen={false}
        actorStatusProvisional={false}
        onOpenRuntimeActor={mocks.open}
      />,
    );
  });
}
const badge = () => host.querySelector<HTMLElement>('span[title*="unread mail"]');
const activity = () => host.querySelector('.lcd-member-ring[data-ring="moving"]');

beforeEach(() => {
  // These cases inspect the LCD ring and its motion.
  setUiStylePrefs({ style: "lcd" });
  vi.useFakeTimers();
  vi.setSystemTime(now);
  mocks.events = [];
  mocks.workingState = "";
  mocks.store = {};
  mocks.open.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  setUiStylePrefs({ style: "classic" });
  host.remove();
  vi.useRealTimers();
});

it("shows the authoritative unread total even when no Mail is loaded", async () => {
  await render();
  expect(badge()?.textContent).toBe("2");
  expect(badge()?.title).toBe("2 unread mail");
  expect(host.querySelector(".sr-only")?.textContent).toContain("2 unread mail");
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(mocks.open).toHaveBeenCalledWith("alpha");
});

it("uses refreshed unread counts instead of stale loaded read markers", async () => {
  mocks.events = [
    {
      id: "old-mail",
      kind: "chat.message",
      ts: "2026-09-26T09:00:00Z",
      by: "user",
      data: { message_mode: "mail", text: "older mail" },
      _read_status: { alpha: false },
    },
  ];
  await render({ unread_count: 123 });
  expect(badge()?.textContent).toBe("99+");
  expect(badge()?.title).toBe("123 unread mail");
  await render({ unread_count: 0 });
  expect(badge()).toBeNull();
});

it("shows confirmation from refreshed Actors and keeps the existing terminal entry", async () => {
  await render({ runtime_confirmation_required: true });
  expect(host.textContent).toContain("actors:runtimeConfirmationPending");
  expect(host.querySelector('button[data-tone="attention"]')).not.toBeNull();
  await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
  expect(mocks.open).toHaveBeenCalledExactlyOnceWith("alpha");
  await render({ runtime_confirmation_required: false });
  expect(host.textContent).not.toContain("actors:runtimeConfirmationPending");
});

it("names a native session by whether it can receive, never as stopped", async () => {
  const native = {
    external_session_id: "codex:thread",
    runner: undefined,
    runtime_state_source: undefined,
  };
  mocks.store = { groupDoc: { group_id: "g1", state: "active" } };
  await render({ ...native, running: false });
  expect(host.textContent).toContain("actors:nativeSession.unavailable");
  expect(host.textContent).not.toContain("Stopped");
  await render({ ...native, running: true });
  expect(host.textContent).toContain("actors:nativeSession.available");
  mocks.store = { groupDoc: { group_id: "g1", state: "paused" } };
  await render({ ...native, running: false });
  expect(host.textContent).toContain("actors:nativeSession.groupPaused");
  // Another Group's snapshot says nothing about this one.
  mocks.store = { groupDoc: { group_id: "other", state: "active" } };
  await render({ ...native, running: false });
  expect(host.textContent).toContain("common:loading");
  expect(host.textContent).not.toContain("Stopped");
});

it("names a closed confirmation only when the daemon reports the pause", async () => {
  await render({ running: false, runtime_confirmation_paused: true });
  expect(host.textContent).toContain("actors:runtimeConfirmationPaused");
  expect(host.querySelector('button[data-tone="attention"]')).not.toBeNull();
  for (const paused of [false, undefined]) {
    await render({ running: false, runtime_confirmation_paused: paused });
    expect(host.textContent).not.toContain("actors:runtimeConfirmationPaused");
    expect(host.querySelector('button[data-tone="attention"]')).toBeNull();
  }
});

it("moves the ring only while work is in progress", async () => {
  for (const state of ["working", "idle", "stuck", "stopped", "working"] as const) {
    await render({ effective_working_state: state, running: state !== "stopped" });
    expect(Boolean(activity())).toBe(state === "working");
  }
});

it("shows no persistent time labels or elapsed-time timers, including for idle Actors", async () => {
  for (const state of ["working", "idle", "stopped"] as const) {
    await render({ effective_working_state: state, running: state !== "stopped" });
    expect(host.textContent).not.toMatch(/\d+[mh]/);
    // A finished turn's brief light is the only timer, and it ends on its own.
    await act(async () => vi.advanceTimersByTime(300));
    expect(vi.getTimerCount()).toBe(0);
  }
});

it("follows locally observed activity without inventing duration from a backend timestamp", async () => {
  mocks.workingState = "idle";
  await render();
  expect(activity()).toBeNull();
  mocks.workingState = "working";
  await render();
  expect(activity()).not.toBeNull();
  expect(host.querySelector("button")?.title).toContain("Working");
});

it("preserves separate unread and queue totals without changing the button size", async () => {
  await render({ runtime: "web_model", unread_count: 123, web_model_queued_count: 120 });
  expect(badge()?.textContent).toBe("99+");
  const queue = host.querySelector<HTMLElement>('span[title*="queued for next turn"]');
  expect(queue?.textContent).toBe("99+");
  expect(queue?.title).toBe("120 queued for next turn");
  const buttonClass = host.querySelector("button")?.className;
  expect(host.querySelector("button")?.title).toContain("123 unread mail");
  expect(host.querySelector("button")?.title).toContain("120 queued for next turn");
  await render({ runtime: "web_model", unread_count: 0, web_model_queued_count: 0 });
  expect(host.querySelector("button")?.className).toBe(buttonClass);
  expect(badge()).toBeNull();
  expect(host.querySelector('span[title*="queued for next turn"]')).toBeNull();
});

it("refreshes counts and activity when switching Groups with the same Actor ID", async () => {
  await render({ runtime: "web_model", web_model_queued_count: 3 });
  await render({ runtime: "web_model", unread_count: 0, effective_working_state: "idle" }, "g2");
  expect(badge()).toBeNull();
  expect(activity()).toBeNull();
  expect(host.querySelector('span[title*="queued for next turn"]')).toBeNull();
});

it.each(["headless", "pty"] as const)(
  "shows a running %s owner error before an older completed preview",
  async (runner) => {
    const card: LiveWorkCard = {
      actorId: actor.id,
      actorLabel: actor.id,
      runtime: actor.runtime!,
      phase: "completed",
      streamPhase: "final_answer",
      text: "Previous result",
      transcriptBlocks: [],
      activities: [],
      updatedAt: "2026-09-26T09:00:00Z",
      streamId: "old-turn",
      pendingEventId: "",
    };
    await render({ runner, runner_effective: runner, effective_working_state: "error" }, "g1", [
      card,
    ]);
    expect(host.querySelector("button")?.title).toContain("Error");
    expect(host.querySelector(".sr-only")?.textContent).toContain("Error");
    expect(host.querySelector("button")?.title).not.toContain("Stopped");
    expect(getRuntimeRingTone({ actor, runner, liveWorkCard: card }, true, "error")).toBe(
      "attention",
    );
    const failedCard = { ...card, phase: "failed" as const };
    await render({ runner, runner_effective: runner, effective_working_state: "working" }, "g1", [
      failedCard,
    ]);
    expect(host.querySelector("button")?.title).toContain("Working");
    expect(activity()).not.toBeNull();
    expect(getRuntimeRingTone({ actor, runner, liveWorkCard: failedCard }, true, "working")).toBe(
      "active",
    );
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(mocks.open).toHaveBeenCalledWith(actor.id);
  },
);

it.each(["headless", "pty"] as const)(
  "shows the recovered %s owner state while retaining older task results",
  async (runner) => {
    const card: LiveWorkCard = {
      actorId: actor.id,
      actorLabel: actor.id,
      runtime: actor.runtime!,
      phase: "failed",
      streamPhase: "final_answer",
      text: "Previous failure",
      transcriptBlocks: [],
      activities: [],
      updatedAt: "2026-09-26T09:00:00Z",
      streamId: "old-turn",
      pendingEventId: "",
    };
    const recovered = {
      ...actor,
      runner,
      runner_effective: runner,
      effective_working_state: "idle",
    };
    await render({ ...recovered, effective_working_state: "error" }, "g1", [card]);
    expect(host.querySelector("button")?.title).toContain("Error");

    for (const phase of ["failed", "pending"] as const) {
      const previous = { ...card, phase };
      await render(recovered, "g1", [previous]);
      expect.soft(host.querySelector("button")?.title).toContain("Running");
      expect.soft(host.querySelector(".sr-only")?.textContent).toContain("Running");
      expect(activity()).toBeNull();
      expect
        .soft(
          getRuntimeRingTone({ actor: recovered, runner, liveWorkCard: previous }, true, "idle"),
        )
        .toBe("idle");
    }
    await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
    expect(mocks.open).toHaveBeenCalledWith(actor.id);
  },
);

it.each([
  ["headless", "waiting", true, "Waiting", "idle"],
  ["pty", "waiting", true, "Waiting", "idle"],
  ["headless", "stopped", false, "Stopped", "stopped"],
  ["pty", "stopped", false, "Stopped", "stopped"],
] as const)(
  "shows the %s owner's current %s state before an older failed preview",
  async (runner, workingState, running, label, tone) => {
    const current = {
      ...actor,
      runner,
      runner_effective: runner,
      running,
      effective_working_state: workingState,
    };
    const card: LiveWorkCard = {
      actorId: actor.id,
      actorLabel: actor.id,
      runtime: actor.runtime!,
      phase: "failed",
      streamPhase: "final_answer",
      text: "Previous failure",
      transcriptBlocks: [],
      activities: [],
      updatedAt: "2026-09-26T09:00:00Z",
      streamId: "old-turn",
      pendingEventId: "",
    };
    await render(current, "g1", [card]);
    expect.soft(host.querySelector("button")?.title).toContain(label);
    expect.soft(host.querySelector(".sr-only")?.textContent).toContain(label);
    expect(activity()).toBeNull();
    expect
      .soft(
        getRuntimeRingTone({ actor: current, runner, liveWorkCard: card }, running, workingState),
      )
      .toBe(tone);
  },
);

it("does not show a retired browser queue badge on a native Grok Actor", async () => {
  await render({ runtime: "grok_web_model", web_model_queued_count: 4, unread_count: 2 });
  expect(host.querySelector('span[title*="queued for next turn"]')).toBeNull();
  expect(host.querySelector("button")?.title).not.toContain("queued for next turn");
  expect(badge()?.textContent).toBe("2");
});
