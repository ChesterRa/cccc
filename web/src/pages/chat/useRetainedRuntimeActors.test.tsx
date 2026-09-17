// @vitest-environment happy-dom
import { act, cloneElement, useCallback } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import type { Actor } from "../../types";
import {
  useRetainedRuntimeActors,
  RETAINED_TERMINAL_TTL_MS,
  type RuntimeActorView,
} from "./useRetainedRuntimeActors";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
const actors: Actor[] = Array.from({ length: 40 }, (_, i) => ({
  id: `a${i}`,
  generation: "original",
  runner: "pty",
  running: true,
}));
type Options = {
  groupId?: string;
  actors?: Actor[];
  visible?: string[];
  groups?: string[];
  loading?: boolean;
  readOnly?: boolean;
};
function Terminal({ identity, isVisible }: RuntimeActorView & { identity: string }) {
  return <textarea data-identity={identity} data-visible={isVisible} />;
}
function Probe({
  groupId = "g1",
  actors: currentActors = actors,
  visible = [],
  groups = ["g1", "g2"],
  loading = false,
  readOnly = false,
}: Options) {
  const renderActor = useCallback(
    (id: string, view: RuntimeActorView) => <Terminal {...view} identity={`${groupId}/${id}`} />,
    [groupId],
  );
  const entries = useRetainedRuntimeActors({
    groupId,
    actors: currentActors,
    visibleActorIds: visible,
    availableGroupIds: groups,
    loading,
    readOnly,
    renderActor,
  });
  return entries.map((entry) =>
    cloneElement(entry.element, { key: entry.key, isVisible: entry.hiddenAt === null }),
  );
}
async function render(options: Options) {
  await act(async () => root.render(<Probe {...options} />));
}
const terminal = (id: string) => host.querySelector<HTMLTextAreaElement>(`[data-identity="${id}"]`);
beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

it("retains visited views by Group and Actor without admitting unvisited Actors", async () => {
  await render({});
  expect(host.children).toHaveLength(0);
  await render({ visible: ["a0"] });
  const first = terminal("g1/a0")!;
  first.value = "selection and local state";
  first.setSelectionRange(2, 8);
  await render({ groupId: "g2", visible: ["a0"] });
  expect(terminal("g2/a0")).not.toBe(first);
  expect(terminal("g1/a0")).toBe(first);
  expect(first.dataset.visible).toBe("false");
  await render({ visible: ["a0"] });
  expect(terminal("g1/a0")).toBe(first);
  expect(first.value).toBe("selection and local state");
  expect(first.selectionStart).toBe(2);
  expect(first.selectionEnd).toBe(8);
});

it("keeps 32 hidden views in addition to visible views and evicts the oldest visit", async () => {
  await render({ visible: ["a0"] });
  const first = terminal("g1/a0");
  for (let i = 1; i <= 32; i++) {
    await act(async () => vi.advanceTimersByTime(1));
    await render({ visible: [`a${i}`] });
  }
  expect(host.children).toHaveLength(33);
  expect(terminal("g1/a0")).toBe(first);
  // Revisit a0: a1, not a0, becomes the oldest hidden view.
  await render({ visible: ["a0"] });
  await act(async () => vi.advanceTimersByTime(1));
  await render({ visible: ["a33", "a34", "a35", "a36"] });
  expect(host.children).toHaveLength(36);
  expect(terminal("g1/a0")).toBe(first);
  expect(terminal("g1/a1")).toBeNull();
  expect(host.querySelectorAll('[data-visible="true"]')).toHaveLength(4);
});

it("expires five minutes after hiding even if metadata updates, but renews on a visit", async () => {
  await render({ visible: ["a0", "a1"] });
  const first = terminal("g1/a0");
  await render({});
  await act(async () => vi.advanceTimersByTime(RETAINED_TERMINAL_TTL_MS - 100));
  await render({
    actors: actors.map((actor) => ({ ...actor, title: "Updated" })),
    visible: ["a1"],
  });
  await render({});
  await act(async () => vi.advanceTimersByTime(100));
  expect(first?.isConnected).toBe(false);
  expect(terminal("g1/a0")).toBeNull();
  expect(terminal("g1/a1")).not.toBeNull();
  await act(async () => vi.advanceTimersByTime(RETAINED_TERMINAL_TTL_MS));
  expect(host.children).toHaveLength(0);
});

it("preserves an intact view through empty hydration and retires removed or recreated Actors", async () => {
  await render({ visible: ["a0", "a1"] });
  const first = terminal("g1/a0");
  const second = terminal("g1/a1");
  await render({ actors: [], visible: ["a0"], loading: true });
  expect(terminal("g1/a0")).toBe(first);
  expect(terminal("g1/a1")).toBe(second);
  await render({ visible: ["a0"] });
  expect(terminal("g1/a0")).toBe(first);
  await render({ actors: [{ ...actors[0], generation: "replacement" }], visible: ["a0"] });
  expect(terminal("g1/a0")).not.toBe(first);
  expect(first?.isConnected).toBe(false);
  expect(second?.isConnected).toBe(false);
});

it("retires hidden Groups on deletion or permission changes", async () => {
  await render({ visible: ["a0"] });
  await render({ groupId: "g2", visible: ["a0"] });
  await render({ groupId: "g2", visible: ["a0"], groups: ["g2"] });
  expect(terminal("g1/a0")).toBeNull();
  await render({ visible: ["a0"] });
  await render({ visible: ["a0"], readOnly: true });
  expect(terminal("g2/a0")).toBeNull();
  expect(terminal("g1/a0")).not.toBeNull();
});

it("does not retain stopped or headless views in the background", async () => {
  const currentActors: Actor[] = [
    { ...actors[0], running: false },
    { ...actors[1], runner: "headless" },
  ];
  await render({ actors: currentActors, visible: ["a0", "a1"] });
  expect(host.children).toHaveLength(2);
  await render({ actors: currentActors });
  expect(host.children).toHaveLength(0);
});
