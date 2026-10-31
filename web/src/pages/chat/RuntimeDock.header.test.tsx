// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../types";
import { RuntimeDock } from "./RuntimeDock";

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  add: vi.fn(),
  visible: 99,
  resize: [] as Array<() => void>,
}));
vi.mock("../../stores", () => ({
  useGroupStore: (select: (state: object) => unknown) => select({}),
  selectChatBucketState: () => ({ events: [] }),
}));
vi.mock("../../hooks/useActorDisplayState", () => ({
  useActorDisplayState: ({ actor }: { actor: Actor }) => ({
    isRunning: actor.running,
    workingState: actor.effective_working_state,
  }),
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
  constructor(callback: () => void) {
    mocks.resize.push(callback);
  }
  observe() {}
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

async function render(actors: Actor[]) {
  await act(async () => {
    root.render(
      <RuntimeDock
        placement="header"
        groupId="g1"
        runtimeActors={actors}
        liveWorkCards={[]}
        isDark={false}
        isSmallScreen={false}
        actorStatusProvisional={false}
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
  host.remove();
  vi.unstubAllGlobals();
  if (offsetTop) Object.defineProperty(HTMLElement.prototype, "offsetTop", offsetTop);
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
