// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useGroupStore } from "../stores";
import { useSSE } from "./useSSE";

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

it("refreshes actor unread on a socket reopening the same initially connected ledger subscription", async () => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  fixtures.sources.length = 0;
  const original = useGroupStore.getState();
  const refreshActors = vi.fn().mockResolvedValue(undefined);
  useGroupStore.setState({ selectedGroupId: "g", actors: [], chatByGroup: {}, refreshActors });
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: false },
      actorsRef: { current: [] },
    });
    return null;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Probe />));
    await act(async () => connection!.connectStream("g"));
    const ledger = fixtures.sources.find((source) => source.url.includes("/ledger/stream"));
    expect(ledger).toBeTruthy();
    await act(async () => {
      ledger!.onopen!();
      await Promise.resolve();
    });
    expect(refreshActors).toHaveBeenCalledWith("g", { includeUnread: true });
    refreshActors.mockClear();
    await act(async () => {
      ledger!.onopen!();
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(refreshActors).toHaveBeenCalledWith("g", { includeUnread: true });
  } finally {
    await act(async () => root.unmount());
    useGroupStore.setState(original, true);
  }
});
