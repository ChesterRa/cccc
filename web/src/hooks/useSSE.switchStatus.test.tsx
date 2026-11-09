// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore, useUIStore } from "../stores";
import { SWITCH_RECONNECTING_GRACE_MS, useSSE } from "./useSSE";

const fixtures = vi.hoisted(() => ({
  sources: [] as Array<{
    url: string;
    onopen: (() => void) | null;
    onerror: (() => void) | null;
    close: () => void;
    addEventListener: () => void;
  }>,
}));
vi.mock("../services/realtime/eventStream", () => ({
  openEventStream: (url: string) => {
    const source = { url, onopen: null, onerror: null, close: vi.fn(), addEventListener: vi.fn() };
    fixtures.sources.push(source);
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
const originalUi = useUIStore.getState();
let connection: ReturnType<typeof useSSE>;
let root: Root;

function ledger(groupId: string) {
  const source = fixtures.sources
    .filter((s) => s.url.includes(`/groups/${groupId}/ledger/stream`))
    .at(-1);
  expect(source).toBeTruthy();
  return source!;
}

const status = () => useUIStore.getState().sseStatus;

beforeEach(async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  vi.useFakeTimers();
  fixtures.sources.length = 0;
  const refreshActors = vi.fn().mockResolvedValue(undefined);
  useGroupStore.setState({ selectedGroupId: "a", actors: [], chatByGroup: {}, refreshActors });
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
  await act(async () => connection.connectStream("a"));
  expect(status()).toBe("connecting");
  await act(async () => ledger("a").onopen!());
  expect(status()).toBe("connected");
});

afterEach(async () => {
  await act(async () => root.unmount());
  vi.useRealTimers();
  useGroupStore.setState(originalGroups, true);
  useUIStore.setState(originalUi, true);
});

// A Group switch runs the lifecycle cleanup and the next connect in one pass.
async function switchTo(groupId: string) {
  await act(async () => {
    useGroupStore.setState({ selectedGroupId: groupId });
    connection.cleanup();
    connection.connectStream(groupId);
  });
}

it("keeps the connection shown as live while a switched-to Group's stream opens in time", async () => {
  await switchTo("b");
  expect(status()).toBe("connected");
  await act(async () => vi.advanceTimersByTime(SWITCH_RECONNECTING_GRACE_MS - 1));
  expect(status()).toBe("connected");
  await act(async () => ledger("b").onopen!());
  await act(async () => vi.advanceTimersByTime(SWITCH_RECONNECTING_GRACE_MS * 2));
  expect(status()).toBe("connected");
});

it("reports reconnecting once a switched-to Group's stream is slower than the grace period", async () => {
  await switchTo("b");
  await act(async () => vi.advanceTimersByTime(SWITCH_RECONNECTING_GRACE_MS));
  expect(status()).toBe("connecting");
  await act(async () => ledger("b").onopen!());
  expect(status()).toBe("connected");
});

it("reports a failed stream at once during a switch", async () => {
  await switchTo("b");
  await act(async () => ledger("b").onerror!());
  expect(status()).toBe("disconnected");
  await act(async () => vi.advanceTimersByTime(SWITCH_RECONNECTING_GRACE_MS * 2));
  expect(status()).toBe("disconnected");
});

it("reports connecting at once for a connect that does not follow a cleanup in the same pass", async () => {
  await act(async () => connection.cleanup());
  expect(status()).toBe("disconnected");
  await act(async () => connection.connectStream("b"));
  expect(status()).toBe("connecting");
});
