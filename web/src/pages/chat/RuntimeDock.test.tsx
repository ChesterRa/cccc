// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor, LedgerEvent } from "../../types";
import { RuntimeDock } from "./RuntimeDock";

const mocks = vi.hoisted(() => ({ events: [] as LedgerEvent[], workingState: "", open: vi.fn() }));
vi.mock("../../stores", () => ({
  useGroupStore: (select: (state: object) => unknown) => select({}),
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
async function render(overrides: Partial<Actor> = {}, groupId = "g1") {
  await act(async () => {
    root.render(
      <RuntimeDock
        groupId={groupId}
        runtimeActors={[{ ...actor, ...overrides }]}
        liveWorkCards={[]}
        isDark={false}
        isSmallScreen={false}
        actorStatusProvisional={false}
        onOpenRuntimeActor={mocks.open}
      />,
    );
  });
}
const badge = () => host.querySelector<HTMLElement>('span[title*="unread mail"]');
const activity = () => host.querySelector(".runtime-dock-ring__activity");

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(now);
  mocks.events = [];
  mocks.workingState = "";
  mocks.open.mockReset();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
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

it("keeps the ring size stable as work starts, idles, needs attention, and stops", async () => {
  for (const state of ["working", "idle", "stuck", "stopped", "working"] as const) {
    await render({ effective_working_state: state, running: state !== "stopped" });
    const ring = host.querySelector(".runtime-dock-ring");
    expect(ring?.getAttribute("width")).toBe("44");
    expect(ring?.getAttribute("height")).toBe("44");
    expect(Boolean(activity())).toBe(state === "working");
  }
});

it("shows no persistent time labels or elapsed-time timers, including for idle Actors", async () => {
  for (const state of ["working", "idle", "stopped"] as const) {
    await render({ effective_working_state: state, running: state !== "stopped" });
    expect(host.textContent).not.toMatch(/\d+[mh]/);
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
  await render({ unread_count: 123, web_model_queued_count: 120 });
  expect(badge()?.textContent).toBe("99+");
  const queue = host.querySelector<HTMLElement>('span[title*="queued for next turn"]');
  expect(queue?.textContent).toBe("99+");
  expect(queue?.title).toBe("120 queued for next turn");
  const buttonClass = host.querySelector("button")?.className;
  expect(host.querySelector("button")?.title).toContain("123 unread mail");
  expect(host.querySelector("button")?.title).toContain("120 queued for next turn");
  await render({ unread_count: 0, web_model_queued_count: 0 });
  expect(host.querySelector("button")?.className).toBe(buttonClass);
  expect(badge()).toBeNull();
  expect(host.querySelector('span[title*="queued for next turn"]')).toBeNull();
});

it("refreshes counts and activity when switching Groups with the same Actor ID", async () => {
  await render({ web_model_queued_count: 3 });
  await render({ unread_count: 0, effective_working_state: "idle" }, "g2");
  expect(badge()).toBeNull();
  expect(activity()).toBeNull();
  expect(host.querySelector('span[title*="queued for next turn"]')).toBeNull();
});
