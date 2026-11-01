// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../types";
import { useTerminalSignalsStore } from "../../stores";
import { setUiStylePrefs } from "../../utils/uiStyle";
import { RuntimeDock } from "./RuntimeDock";
import type { LiveWorkCard } from "./liveWorkCards";

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  add: vi.fn(),
  visible: 99,
  resize: [] as Array<() => void>,
}));
vi.mock("../../stores", async () => ({
  ...(await vi.importActual<typeof import("../../stores/useTerminalSignalsStore")>(
    "../../stores/useTerminalSignalsStore",
  )),
  useGroupStore: (select: (state: object) => unknown) => select({}),
  selectChatBucketState: () => ({ events: [] }),
}));
vi.mock("../../components/ActorAvatar", () => ({ ActorAvatar: () => <span>avatar</span> }));
vi.mock("./RuntimeDockTicker", () => ({ RuntimeDockTicker: () => null }));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, options?: Record<string, unknown>) =>
      options?.defaultValue
        ? String(options.defaultValue)
        : options?.count !== undefined
          ? `${key}:${String(options.count)}`
          : key,
  }),
}));

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

// happy-dom has no layout: chips past `mocks.visible` report a second line.
const offsetTop = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetTop");
class FakeResizeObserver {
  constructor(private callback: ResizeObserverCallback) {}
  observe(element: Element) {
    if (element.classList.contains("lcd-members-row")) {
      mocks.resize.push(() => this.callback([], this as unknown as ResizeObserver));
    }
  }
  unobserve() {}
  disconnect() {}
}

let host: HTMLDivElement;
let root: Root;
const actor = (id: string, overrides: Partial<Actor> = {}): Actor => ({
  id,
  title: id,
  runtime: "codex",
  runner: "pty",
  runtime_state_source: "managed_session",
  enabled: true,
  running: true,
  effective_working_state: "idle",
  unread_count: 0,
  ...overrides,
});

async function render(
  actors: Actor[],
  groupId = "g1",
  liveWorkCards: LiveWorkCard[] = [],
  actorStatusProvisional = false,
  motionVisible = true,
) {
  await act(async () => {
    root.render(
      <RuntimeDock
        placement="row"
        groupId={groupId}
        runtimeActors={actors}
        liveWorkCards={liveWorkCards}
        isDark={false}
        isSmallScreen={false}
        motionVisible={motionVisible}
        actorStatusProvisional={actorStatusProvisional}
        onAddAgent={mocks.add}
        onOpenRuntimeActor={mocks.open}
      />,
    );
  });
}
const relayout = (visible: number) =>
  act(async () => {
    mocks.visible = visible;
    mocks.resize.forEach((callback) => callback());
  });
const chips = () => [...host.querySelectorAll<HTMLElement>("[data-lcd-member-chip]")];
const more = () => host.querySelector<HTMLButtonElement>(".lcd-member-more");

beforeEach(() => {
  mocks.open.mockReset();
  mocks.add.mockReset();
  mocks.visible = 99;
  mocks.resize = [];
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
  Object.defineProperty(HTMLElement.prototype, "offsetTop", {
    configurable: true,
    get(this: HTMLElement) {
      if (!this.hasAttribute("data-lcd-member-chip")) return 0;
      const index = Array.prototype.indexOf.call(this.parentElement?.children ?? [], this);
      return index < mocks.visible ? 0 : 28;
    },
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  act(() => setUiStylePrefs({ style: "classic" }));
  host.remove();
  vi.unstubAllGlobals();
  if (offsetTop) Object.defineProperty(HTMLElement.prototype, "offsetTop", offsetTop);
});

it("flashes attention only on a live transition, never on mount or group switch", async () => {
  vi.useFakeTimers();
  try {
    act(() => setUiStylePrefs({ style: "lcd" }));
    const idle = actor("alpha");
    const attention = { ...idle, effective_working_state: "stuck" };
    await render([attention]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
    await render([idle]);
    await render([attention]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(true);
    await act(async () => vi.advanceTimersByTime(1500));
    expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
    await render([attention]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
    await render([attention], "g2");
    expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
    await render([attention], "g1");
    expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
  } finally {
    vi.useRealTimers();
  }
});

it("treats initial status hydration as a baseline rather than a new attention event", async () => {
  act(() => setUiStylePrefs({ style: "lcd" }));
  const idle = actor("alpha");
  await render([idle], "g1", [], true);
  await render([{ ...idle, effective_working_state: "error" }], "g1", [], false);
  expect(chips()[0].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
});

it("flashes +N for a newly attentive hidden member, not one already attentive at mount", async () => {
  vi.useFakeTimers();
  try {
    act(() => setUiStylePrefs({ style: "lcd" }));
    const alpha = actor("alpha");
    const idle = actor("beta");
    const attention = { ...idle, effective_working_state: "error" };
    await render([alpha, attention]);
    await relayout(1);
    expect(more()?.dataset.tone).toBe("attention");
    expect(more()?.hasAttribute("data-attention-flash")).toBe(false);
    await render([alpha, idle]);
    await render([alpha, attention]);
    expect(more()?.hasAttribute("data-attention-flash")).toBe(true);
    expect(chips()[1].querySelector("button")?.hasAttribute("data-attention-flash")).toBe(false);
    await act(async () => vi.advanceTimersByTime(1500));
    expect(more()?.hasAttribute("data-attention-flash")).toBe(false);
    await render([alpha, attention]);
    expect(more()?.hasAttribute("data-attention-flash")).toBe(false);
  } finally {
    vi.useRealTimers();
  }
});

it("moves only a visible working member and briefly marks a working-to-idle transition", async () => {
  vi.useFakeTimers();
  try {
    act(() => setUiStylePrefs({ style: "lcd" }));
    const idle = actor("alpha");
    const working = { ...idle, effective_working_state: "working" };
    await render([working, actor("beta", { effective_working_state: "working" })]);
    expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("moving");
    await relayout(1);
    expect(chips()[1].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("still");
    await render([idle, actor("beta", { effective_working_state: "working" })]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-flourish")).toBe(true);
    expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("still");
    await act(async () => vi.advanceTimersByTime(250));
    expect(chips()[0].querySelector("button")?.hasAttribute("data-flourish")).toBe(false);
    expect(chips()[0].querySelector(".lcd-member-ring")).toBeNull();
  } finally {
    vi.useRealTimers();
  }
});

it("stills a hidden dock and resumes working rings when it returns", async () => {
  act(() => setUiStylePrefs({ style: "lcd" }));
  const working = actor("alpha", { effective_working_state: "working" });
  await render([working]);
  expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("moving");
  await render([working], "g1", [], false, false);
  expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("still");
  await render([working], "g1", [], false, true);
  expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("moving");
});

it("does not animate queued work or celebrate the queue becoming empty", async () => {
  vi.useFakeTimers();
  try {
    act(() => setUiStylePrefs({ style: "lcd" }));
    const unmanaged = actor("alpha", { runtime_state_source: "terminal" });
    const pending: LiveWorkCard = {
      actorId: "alpha",
      actorLabel: "alpha",
      runtime: "codex",
      phase: "pending",
      streamPhase: "",
      text: "",
      transcriptBlocks: [],
      activities: [],
      updatedAt: "",
      streamId: "",
      pendingEventId: "p1",
    };
    await render([unmanaged], "g1", [pending]);
    expect(chips()[0].querySelector("button")?.dataset.tone).toBe("active");
    expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("still");
    await render([unmanaged], "g1", [{ ...pending, phase: "streaming" }]);
    expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("moving");
    await render([unmanaged], "g1", [pending]);
    expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("still");
    await render([unmanaged]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-flourish")).toBe(false);
    await render([unmanaged], "g1", [{ ...pending, phase: "streaming" }]);
    await render([unmanaged], "g1", [{ ...pending, phase: "completed" }]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-flourish")).toBe(true);
    await act(async () => vi.advanceTimersByTime(250));
    await render([unmanaged], "g1", [{ ...pending, phase: "streaming" }]);
    await render([unmanaged], "g1", [
      { ...pending, phase: "completed", pendingEventId: "another-task" },
    ]);
    expect(chips()[0].querySelector("button")?.hasAttribute("data-flourish")).toBe(false);
  } finally {
    vi.useRealTimers();
  }
});

it("does not celebrate a quiet terminal signal expiring as completed work", async () => {
  vi.useFakeTimers();
  try {
    act(() => setUiStylePrefs({ style: "lcd" }));
    const unmanaged = actor("alpha", { runtime_state_source: "terminal" });
    await render([unmanaged]);
    await act(async () => {
      useTerminalSignalsStore
        .getState()
        .setSignal("g1", "alpha", { kind: "working_output", updatedAt: Date.now() });
    });
    expect(chips()[0].querySelector(".lcd-member-ring")?.getAttribute("data-ring")).toBe("moving");
    await act(async () => vi.advanceTimersByTime(5_051));
    expect(chips()[0].querySelector("button")?.dataset.tone).toBe("idle");
    expect(chips()[0].querySelector("button")?.hasAttribute("data-flourish")).toBe(false);
  } finally {
    useTerminalSignalsStore.getState().clearSignal("g1", "alpha");
    vi.useRealTimers();
  }
});

it("keeps a running ring's phase across renders and realigns it when it reappears", async () => {
  act(() => setUiStylePrefs({ style: "lcd" }));
  const originalTimeline = Object.getOwnPropertyDescriptor(document, "timeline");
  let timelineTime = 500;
  const motionListeners = new Set<EventListenerOrEventListenerObject>();
  const matchMedia = vi
    .spyOn(window, "matchMedia")
    .mockImplementation(
      (query) =>
        ({
          matches: false,
          media: query,
          addEventListener: (_type: string, listener: EventListenerOrEventListenerObject) =>
            motionListeners.add(listener),
          removeEventListener: (_type: string, listener: EventListenerOrEventListenerObject) =>
            motionListeners.delete(listener),
        }) as unknown as MediaQueryList,
    );
  Object.defineProperty(document, "timeline", {
    configurable: true,
    value: {
      get currentTime() {
        return timelineTime;
      },
    },
  });
  try {
    const alpha = actor("alpha");
    const beta = actor("beta", { effective_working_state: "working" });
    const delay = () =>
      chips()[1]
        .querySelector<HTMLElement>(".lcd-member-ring")
        ?.style.getPropertyValue("--lcd-ring-delay");
    await render([alpha, beta]);
    expect(delay()).toBe("-500ms");
    timelineTime = 1000;
    await render([alpha, { ...beta, unread_count: 1 }]);
    expect(delay()).toBe("-500ms");
    await relayout(1);
    timelineTime = 1250;
    await relayout(99);
    expect(delay()).toBe("-1250ms");
    timelineTime = 1500;
    await act(async () => {
      for (const listener of motionListeners) {
        if (typeof listener === "function") listener(new Event("change"));
        else listener.handleEvent(new Event("change"));
      }
    });
    expect(delay()).toBe("-1500ms");
    await render([alpha, { ...beta, effective_working_state: "idle" }]);
    expect(motionListeners.size).toBe(0);
  } finally {
    matchMedia.mockRestore();
    if (originalTimeline) Object.defineProperty(document, "timeline", originalTimeline);
    else Reflect.deleteProperty(document, "timeline");
  }
});

it("shows each member as a tile whose state is in its tone and accessible text, and keeps open and add", async () => {
  await render([
    actor("alpha", { effective_working_state: "working" }),
    actor("beta", { running: false }),
    actor("gamma", { effective_working_state: "stuck" }),
  ]);
  const buttons = chips().map((chip) => chip.querySelector<HTMLButtonElement>("button")!);
  expect(buttons.map((button) => button.dataset.tone)).toEqual(["active", "stopped", "attention"]);
  // No visible words beside the tiles; the state is in the description.
  expect(host.querySelector(".lcd-member-name")).toBeNull();
  const descriptions = buttons.map(
    (button) => document.getElementById(button.getAttribute("aria-describedby")!)?.textContent,
  );
  expect(descriptions).toEqual([
    expect.stringContaining("Working"),
    expect.stringContaining("Stopped"),
    expect.stringContaining("Stuck"),
  ]);
  // One string joined by real middle dots, not escape text left in JSX.
  expect(descriptions[0]).toMatch(/^alpha · \S+ · /);
  expect(descriptions.join("")).not.toContain("\\u00B7");
  expect(more()).toBeNull();
  await act(async () => buttons[1].click());
  expect(mocks.open).toHaveBeenCalledWith("beta");
  await act(async () => host.querySelector<HTMLButtonElement>(".lcd-member-add")!.click());
  expect(mocks.add).toHaveBeenCalledTimes(1);
});

it("moves members that do not fit behind +N and says which have unread Mail or queued work", async () => {
  await render([
    actor("alpha", { unread_count: 4 }),
    actor("beta"),
    actor("gamma", { unread_count: 1 }),
    actor("delta", { runtime: "web_model", web_model_queued_count: 2 }),
  ]);
  await relayout(2);
  const hidden = chips().filter((chip) => chip.getAttribute("aria-hidden") === "true");
  expect(hidden.map((chip) => chip.hasAttribute("inert"))).toEqual([true, true]);
  expect(more()?.textContent).toBe("+2");
  // Alpha's visible Mail is not counted among the hidden members.
  expect(more()?.getAttribute("aria-label")).toBe(
    "runtimeDockMoreMembers:2 · runtimeDockMoreMembersMail:1 · runtimeDockMoreMembersQueued:1",
  );

  await relayout(99);
  expect(more()).toBeNull();
  expect(chips().some((chip) => chip.hasAttribute("inert"))).toBe(false);
});

it("lists the hidden members in +N and opens the chosen one", async () => {
  await render([actor("alpha"), actor("beta"), actor("gamma")]);
  await relayout(1);
  await act(async () => more()!.click());
  const list = document.body.querySelector<HTMLElement>(".lcd-members-popover");
  expect(list?.hasAttribute("data-lcd-scope")).toBe(true);
  const listed = [...list!.querySelectorAll<HTMLButtonElement>("button")];
  expect(listed.map((button) => button.getAttribute("aria-label"))).toEqual([
    "Open terminal for beta",
    "Open terminal for gamma",
  ]);
  // The list has room for names; the strip does not show them.
  expect(listed.map((button) => button.querySelector(".lcd-member-name")?.textContent)).toEqual([
    "beta",
    "gamma",
  ]);
  const ids = [...document.querySelectorAll("[id]")].map((element) => element.id);
  expect(new Set(ids).size).toBe(ids.length);
  await act(async () => listed[1].click());
  expect(mocks.open).toHaveBeenCalledWith("gamma");
  expect(document.body.querySelector(".lcd-members-popover")).toBeNull();
});

it("keeps Add outside the scrolling member strip on phones, and preserves the desktop placement", async () => {
  const renderDock = async (isSmallScreen: boolean, readOnly = false) => {
    await act(async () => {
      root.render(
        <RuntimeDock
          groupId="g1"
          runtimeActors={[actor("alpha"), actor("beta"), actor("gamma")]}
          liveWorkCards={[]}
          isDark={false}
          isSmallScreen={isSmallScreen}
          readOnly={readOnly}
          actorStatusProvisional={false}
          onAddAgent={mocks.add}
          onOpenRuntimeActor={mocks.open}
        />,
      );
    });
  };
  await renderDock(true);
  let strip = host.querySelector<HTMLElement>("[data-runtime-dock-strip]")!;
  let add = host.querySelector<HTMLButtonElement>("[data-lcd-dock-add]")!;
  expect(strip.contains(add)).toBe(false);
  expect(strip.querySelectorAll("[data-lcd-dock-button]")).toHaveLength(3);
  await act(async () => add.click());
  expect(mocks.add).toHaveBeenCalledOnce();

  await renderDock(false);
  strip = host.querySelector<HTMLElement>("[data-runtime-dock-strip]")!;
  add = host.querySelector<HTMLButtonElement>("[data-lcd-dock-add]")!;
  expect(strip.contains(add)).toBe(true);

  await renderDock(true, true);
  expect(host.querySelector("[data-lcd-dock-add]")).toBeNull();
});

it("summarizes only hidden attention and updates when its actor changes state", async () => {
  const alpha = actor("alpha", { effective_working_state: "stuck" });
  const beta = actor("beta");
  const gamma = actor("gamma", {
    unread_count: 1,
    web_model_queued_count: 2,
    runtime: "web_model",
  });
  await render([alpha, beta, gamma]);
  await relayout(1);
  expect(more()?.dataset.tone).toBeUndefined();
  expect(more()?.getAttribute("aria-label")).not.toContain("runtimeDockMoreMembersAttention");

  await render([alpha, beta, { ...gamma, effective_working_state: "error" }]);
  expect(more()?.dataset.tone).toBe("attention");
  expect(more()?.getAttribute("aria-label")).toBe(
    "runtimeDockMoreMembers:2 · runtimeDockMoreMembersMail:1 · runtimeDockMoreMembersQueued:1 · runtimeDockMoreMembersAttention:1",
  );

  await render([alpha, beta, gamma]);
  expect(more()?.dataset.tone).toBeUndefined();
  expect(more()?.getAttribute("aria-label")).not.toContain("runtimeDockMoreMembersAttention");
});

it("does not carry a hidden member's attention into another group", async () => {
  await render([actor("alpha"), actor("beta", { effective_working_state: "error" })]);
  await relayout(1);
  expect(more()?.dataset.tone).toBe("attention");
  await render([actor("alpha"), actor("beta")], "g2");
  await relayout(1);
  expect(more()?.dataset.tone).toBeUndefined();
  expect(more()?.getAttribute("aria-label")).not.toContain("runtimeDockMoreMembersAttention");
});

it("follows a hidden member's browser terminal signal without changing its actor props", async () => {
  vi.useFakeTimers();
  try {
    const unmanaged = actor("beta", {
      runtime_state_source: "external_session",
      effective_working_state: "error",
    });
    await render([actor("alpha"), unmanaged]);
    await relayout(1);
    expect(more()?.dataset.tone).toBe("attention");
    await act(async () => {
      useTerminalSignalsStore
        .getState()
        .setSignal("g1", "beta", { kind: "working_output", updatedAt: Date.now() });
    });
    expect(more()?.dataset.tone).toBeUndefined();
    await act(async () => vi.advanceTimersByTime(5_051));
    expect(more()?.dataset.tone).toBe("attention");
  } finally {
    useTerminalSignalsStore.getState().clearSignal("g1", "beta");
    vi.useRealTimers();
  }
});

it("moves focus to the overflow button only if the focused member is folded", async () => {
  await render([actor("alpha"), actor("beta"), actor("gamma")]);
  const external = document.createElement("button");
  document.body.append(external);
  try {
    await act(async () => chips()[2].querySelector<HTMLButtonElement>("button")!.focus());
    await relayout(1);
    expect(document.activeElement).toBe(more());
    await act(async () => external.focus());
    await relayout(2);
    expect(document.activeElement).toBe(external);
  } finally {
    external.remove();
  }
});

it("restores focus to a revealed member when the overflow button disappears", async () => {
  await render([actor("alpha"), actor("beta"), actor("gamma")]);
  await relayout(1);
  await act(async () => more()!.focus());
  await relayout(99);
  expect(more()).toBeNull();
  expect(document.activeElement).toBe(chips()[1].querySelector("button"));
});

it("restores the focused popover member when it becomes visible and does not reopen the popover", async () => {
  await render([actor("alpha"), actor("beta"), actor("gamma")]);
  await relayout(1);
  await act(async () => more()!.click());
  const listed = document.body.querySelectorAll<HTMLButtonElement>(".lcd-members-popover button");
  await act(async () => listed[1].focus());
  await relayout(99);
  expect(more()).toBeNull();
  expect(document.activeElement).toBe(chips()[2].querySelector("button"));
  await relayout(1);
  expect(document.body.querySelector(".lcd-members-popover")).toBeNull();
});

it("restores a focused popover member when only some hidden members become visible", async () => {
  await render([actor("alpha"), actor("beta"), actor("gamma")]);
  await relayout(1);
  await act(async () => more()!.click());
  const listed = document.body.querySelectorAll<HTMLButtonElement>(".lcd-members-popover button");
  await act(async () => listed[0].focus());
  await relayout(2);
  expect(more()?.textContent).toBe("+1");
  expect(document.body.querySelector(".lcd-members-popover")).toBeNull();
  expect(document.activeElement).toBe(chips()[1].querySelector("button"));
});

it("does not reopen the overflow popover after all hidden members are removed", async () => {
  const alpha = actor("alpha"),
    beta = actor("beta"),
    gamma = actor("gamma");
  await render([alpha, beta, gamma]);
  await relayout(1);
  await act(async () => more()!.click());
  expect(document.body.querySelector(".lcd-members-popover")).not.toBeNull();
  await render([alpha]);
  expect(more()).toBeNull();
  await render([alpha, beta, gamma]);
  await relayout(1);
  expect(more()?.textContent).toBe("+2");
  expect(document.body.querySelector(".lcd-members-popover")).toBeNull();
});

it("restores all members when they fit after releasing the overflow button's width", async () => {
  await render([actor("a"), actor("b"), actor("c"), actor("d"), actor("e")]);
  await relayout(4);
  expect(more()?.textContent).toBe("+1");
  const row = host.querySelector<HTMLElement>(".lcd-members-row")!;
  row.style.columnGap = "4px";
  row.parentElement!.style.columnGap = "4px";
  vi.spyOn(row, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ width: 158 }));
  vi.spyOn(more()!, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ width: 32 }));
  for (const chip of chips()) {
    vi.spyOn(chip, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ width: 34 }));
    vi.spyOn(chip.querySelector("button")!, "getBoundingClientRect").mockReturnValue(
      DOMRect.fromRect({ width: 34 }),
    );
  }
  // 158px current row + 32px overflow + 4px gap = 194px available.
  // Five 34px tiles and four 4px gaps need 186px, so overflow is unnecessary.
  await relayout(4);
  expect(more()).toBeNull();
  expect(chips().every((chip) => !chip.hasAttribute("inert"))).toBe(true);
});

it("keeps overflow when the available width falls a fraction of a pixel short", async () => {
  await render([actor("a"), actor("b"), actor("c"), actor("d"), actor("e")]);
  await relayout(4);
  const row = host.querySelector<HTMLElement>(".lcd-members-row")!;
  row.style.columnGap = "4px";
  row.parentElement!.style.columnGap = "4px";
  vi.spyOn(row, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ width: 149.75 }));
  vi.spyOn(more()!, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ width: 32 }));
  for (const chip of chips()) {
    vi.spyOn(chip, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ width: 34 }));
    vi.spyOn(chip.querySelector("button")!, "getBoundingClientRect").mockReturnValue(
      DOMRect.fromRect({ width: 34 }),
    );
  }
  // 185.75px available is less than the 186px needed by the five tiles.
  await relayout(4);
  expect(more()?.textContent).toBe("+1");
  expect(chips()[4].hasAttribute("inert")).toBe(true);
});

it("moves a clipped first member into overflow and keeps keyboard focus", async () => {
  const attention = actor("a", { effective_working_state: "stuck" });
  await render([attention, actor("b"), actor("c")]);
  await relayout(1);
  expect(more()?.textContent).toBe("+2");
  const row = host.querySelector<HTMLElement>(".lcd-members-row")!;
  const first = chips()[0].querySelector<HTMLButtonElement>("button")!;
  await act(async () => first.focus());
  vi.spyOn(row, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ x: 100, width: 12 }));
  vi.spyOn(first, "getBoundingClientRect").mockReturnValue(DOMRect.fromRect({ x: 100, width: 40 }));

  await relayout(1);
  expect(chips().every((chip) => chip.hasAttribute("inert"))).toBe(true);
  expect(more()?.textContent).toBe("+3");
  expect(more()?.getAttribute("aria-label")).toContain("runtimeDockMoreMembersAttention:1");
  expect(document.activeElement).toBe(more());
});
