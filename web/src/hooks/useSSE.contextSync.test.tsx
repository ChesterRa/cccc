// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../services/api";
import { useGroupStore } from "../stores";
import { useSSE } from "./useSSE";

const fixtures = vi.hoisted(() => ({
  listeners: new Map<string, (event: { data: string }) => void>(),
  source: null as { onopen: (() => void) | null } | null,
}));
vi.mock("../services/realtime/eventStream", () => ({
  openEventStream: (url: string) => {
    const source = {
      url,
      onopen: null,
      onerror: null,
      close: vi.fn(),
      addEventListener: (name: string, listener: (event: { data: string }) => void) => {
        if (url.includes("/ledger/stream")) fixtures.listeners.set(name, listener);
      },
    };
    if (url.includes("/ledger/stream")) fixtures.source = source;
    return source;
  },
}));
vi.mock("./sse/reconcileLedgerTail", () => ({
  reconcileLedgerTail: vi.fn().mockResolvedValue(""),
}));
vi.mock("../services/api", () => ({
  withAuthToken: (path: string) => path,
  fetchGroup: vi.fn().mockResolvedValue({ ok: false }),
  fetchContext: vi.fn().mockResolvedValue({ ok: false }),
  invalidateContextRead: vi.fn(),
}));

const originalGroups = useGroupStore.getState();
const originalClearTimeout = window.clearTimeout;
let root: Root;

function contextSync(id: string) {
  const listener = fixtures.listeners.get("ledger");
  expect(listener).toBeTruthy();
  const event = {
    id,
    kind: "context.sync",
    group_id: "a",
    data: { changes: [{ op: "task.create" }] },
  };
  listener!({ data: JSON.stringify(event) });
}

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  // Chrome rejects window.clearTimeout called on another object ("Illegal
  // invocation"); the fake timer enforces the same rule.
  const fakeClear = window.clearTimeout;
  window.clearTimeout = function (this: unknown, id?: number) {
    if (this !== undefined && this !== window && this !== globalThis) {
      throw new TypeError("Illegal invocation");
    }
    return fakeClear(id);
  } as typeof window.clearTimeout;
  fixtures.listeners.clear();
  vi.mocked(api.fetchContext).mockClear();
  useGroupStore.setState({
    selectedGroupId: "a",
    actors: [],
    chatByGroup: {},
    refreshActors: vi.fn().mockResolvedValue(undefined),
  });
  let connection: ReturnType<typeof useSSE> | null = null;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: false },
      actorsRef: { current: [] },
    });
    return null;
  }
  root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => connection!.connectStream("a"));
  await act(async () => fixtures.source!.onopen!());
});

afterEach(async () => {
  await act(async () => root.unmount());
  vi.useRealTimers();
  window.clearTimeout = originalClearTimeout;
  useGroupStore.setState(originalGroups, true);
});

it("reads the context again for every context change made elsewhere", async () => {
  const overviewReads = () =>
    vi
      .mocked(api.fetchContext)
      .mock.calls.filter(([gid, opts]) => gid === "a" && opts?.detail === "overview").length;
  const before = overviewReads();

  await act(async () => contextSync("e1"));
  await act(async () => vi.advanceTimersByTime(150));
  expect(overviewReads()).toBe(before + 1);

  // The second change replaces the timer the first one left behind.
  await act(async () => contextSync("e2"));
  await act(async () => vi.advanceTimersByTime(150));
  expect(overviewReads()).toBe(before + 2);
});
