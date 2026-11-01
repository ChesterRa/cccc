// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore } from "../stores";
import { useSSE } from "./useSSE";
import { reconcileLedgerTail } from "./sse/reconcileLedgerTail";

type FixtureSource = {
  url: string;
  groupId?: string;
  readyState: number;
  onopen: (() => void) | null;
  onerror: (() => void) | null;
  close: () => void;
  addEventListener: (type: string, cb: (event: Event) => void) => void;
  listeners: Map<string, (event: Event) => void>;
  transport: {
    reconnect: ReturnType<typeof vi.fn>;
    reconnectIfStale: ReturnType<typeof vi.fn>;
    retryPendingSocket: ReturnType<typeof vi.fn>;
  };
};
const fixture = vi.hoisted(() => ({ sources: [] as FixtureSource[], fetchBoundary: vi.fn() }));
vi.mock("../services/realtime/eventStream", () => ({
  openEventStream: (url: string) => {
    const listeners = new Map<string, (event: Event) => void>();
    const source: FixtureSource = {
      url,
      groupId: url.match(/\/groups\/([^/]+)\//)?.[1],
      readyState: 0,
      onopen: null,
      onerror: null,
      close: vi.fn(),
      addEventListener: (type, cb) => {
        listeners.set(type, cb);
      },
      listeners,
      transport: {
        reconnect: vi.fn(),
        reconnectIfStale: vi.fn().mockReturnValue(false),
        retryPendingSocket: vi.fn().mockReturnValue(true),
      },
    };
    fixture.sources.push(source);
    return source;
  },
}));
vi.mock("./sse/reconcileLedgerTail", () => ({
  reconcileLedgerTail: vi.fn().mockResolvedValue("anchor"),
}));
vi.mock("../services/api", () => ({
  withAuthToken: (path: string) => path,
  fetchGroup: vi.fn().mockResolvedValue({ ok: false }),
  fetchContext: vi.fn().mockResolvedValue({ ok: false }),
  fetchLedgerBoundary: fixture.fetchBoundary,
  invalidateContextRead: vi.fn(),
}));

beforeEach(() => {
  fixture.sources.length = 0;
  fixture.fetchBoundary.mockReset();
});

async function mount() {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const original = useGroupStore.getState();
  useGroupStore.setState({
    selectedGroupId: "g",
    actors: [],
    chatByGroup: {},
    refreshActors: vi.fn().mockResolvedValue(undefined),
  });
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: { current: false },
      actorsRef: { current: [] },
    });
    return null;
  }
  const root = createRoot(document.createElement("div"));
  await act(async () => root.render(<Probe />));
  await act(async () => connection!.connectStream("g"));
  const ledger = fixture.sources.find((source) => source.url.includes("/ledger/stream"))!;
  ledger.readyState = 1;
  await act(async () => {
    ledger.onopen!();
    await Promise.resolve();
  });
  return {
    ledger,
    close: async () => {
      await act(async () => root.unmount());
      useGroupStore.setState(original, true);
    },
  };
}

function boundary(id: string) {
  return { ok: true, result: { events: [{ id }], has_more: true, count: 1 } };
}

it("checks a stable boundary on focus without reconnecting a healthy stream", async () => {
  const { ledger, close } = await mount();
  fixture.fetchBoundary.mockResolvedValue(boundary("anchor"));
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(fixture.fetchBoundary).toHaveBeenCalledOnce();
    expect(ledger.transport.reconnect).not.toHaveBeenCalled();
  } finally {
    await close();
  }
});

it("reconnects a stale stream on focus without a boundary fetch", async () => {
  const { ledger, close } = await mount();
  ledger.transport.reconnectIfStale.mockReturnValue(true);
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(ledger.transport.reconnectIfStale).toHaveBeenCalledWith(25000);
    expect(fixture.fetchBoundary).not.toHaveBeenCalled();
  } finally {
    await close();
  }
});

it("starts a pending socket retry once on return, without probing a disconnected source", async () => {
  const { ledger, close } = await mount();
  ledger.readyState = 0;
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });
    expect(ledger.transport.retryPendingSocket).toHaveBeenCalledOnce();
    expect(fixture.fetchBoundary).not.toHaveBeenCalled();
    expect(ledger.transport.reconnectIfStale).not.toHaveBeenCalled();
  } finally {
    await close();
  }
});

it("does not start a retry for a hidden page or a departed group", async () => {
  const { ledger, close } = await mount();
  const hidden = Object.getOwnPropertyDescriptor(document, "hidden");
  ledger.readyState = 0;
  try {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(ledger.transport.retryPendingSocket).not.toHaveBeenCalled();
    Object.defineProperty(document, "hidden", { configurable: true, get: () => false });
    await act(async () => useGroupStore.setState({ selectedGroupId: "other" }));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(ledger.transport.retryPendingSocket).not.toHaveBeenCalled();
  } finally {
    if (hidden) Object.defineProperty(document, "hidden", hidden);
    else Reflect.deleteProperty(document, "hidden");
    await close();
  }
});

it("reconnects a suspected missed boundary once and coalesces simultaneous focus events", async () => {
  const { ledger, close } = await mount();
  let resolve!: (value: unknown) => void;
  fixture.fetchBoundary.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      window.dispatchEvent(new Event("focus"));
    });
    expect(fixture.fetchBoundary).toHaveBeenCalledOnce();
    await act(async () => {
      resolve(boundary("missed"));
      await Promise.resolve();
    });
    expect(ledger.transport.reconnect).toHaveBeenCalledOnce();
  } finally {
    await close();
  }
});

it("checks once when a hidden page becomes visible, even without a focus event", async () => {
  const { ledger, close } = await mount();
  const hidden = Object.getOwnPropertyDescriptor(document, "hidden");
  let isHidden = true;
  Object.defineProperty(document, "hidden", { configurable: true, get: () => isHidden });
  fixture.fetchBoundary.mockResolvedValue(boundary("missed"));
  try {
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
      isHidden = false;
      document.dispatchEvent(new Event("visibilitychange"));
      await Promise.resolve();
    });
    expect(fixture.fetchBoundary).toHaveBeenCalledOnce();
    expect(ledger.transport.reconnect).toHaveBeenCalledOnce();
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(fixture.fetchBoundary).toHaveBeenCalledOnce();
    expect(ledger.transport.reconnect).toHaveBeenCalledOnce();
  } finally {
    if (hidden) Object.defineProperty(document, "hidden", hidden);
    else Reflect.deleteProperty(document, "hidden");
    await close();
  }
});

it("does not reconnect if live delivery advances the cursor during the HTTP read", async () => {
  const { ledger, close } = await mount();
  let resolve!: (value: unknown) => void;
  fixture.fetchBoundary.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    await act(async () => {
      ledger.listeners.get("ledger")!(
        new MessageEvent("ledger", {
          data: JSON.stringify({
            id: "live",
            kind: "group.started",
            by: "system",
            group_id: "g",
            data: {},
          }),
        }),
      );
      resolve(boundary("missed"));
      await Promise.resolve();
    });
    expect(ledger.transport.reconnect).not.toHaveBeenCalled();
  } finally {
    await close();
  }
});

it("ignores a completed boundary check after switching Groups", async () => {
  const { ledger, close } = await mount();
  let resolve!: (value: unknown) => void;
  fixture.fetchBoundary.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    await act(async () => {
      useGroupStore.setState({ selectedGroupId: "other" });
    });
    await act(async () => {
      resolve(boundary("missed"));
      await Promise.resolve();
    });
    expect(ledger.transport.reconnect).not.toHaveBeenCalled();
  } finally {
    await close();
  }
});

it("does not reconnect on a failed boundary read", async () => {
  const { ledger, close } = await mount();
  fixture.fetchBoundary.mockResolvedValue({
    ok: false,
    error: { code: "NETWORK_ERROR", message: "offline" },
  });
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(ledger.transport.reconnect).not.toHaveBeenCalled();
  } finally {
    await close();
  }
});

it("does not launch a second boundary check while initial reconciliation is in flight", async () => {
  let release!: (cursor: string) => void;
  vi.mocked(reconcileLedgerTail).mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  const { ledger, close } = await mount();
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(fixture.fetchBoundary).not.toHaveBeenCalled();
    expect(ledger.transport.reconnect).not.toHaveBeenCalled();
    await act(async () => {
      release("anchor");
      await Promise.resolve();
    });
  } finally {
    await close();
  }
});

it("aborts a stalled focus check after five seconds so a later focus can retry", async () => {
  const { ledger, close } = await mount();
  vi.useFakeTimers();
  fixture.fetchBoundary.mockImplementation(
    (_group: string, signal: AbortSignal) =>
      new Promise((resolve) => {
        signal.addEventListener("abort", () =>
          resolve({ ok: false, error: { code: "NETWORK_ERROR" } }),
        );
      }),
  );
  try {
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(fixture.fetchBoundary.mock.calls[0][1].aborted).toBe(true);
    expect(ledger.transport.reconnect).not.toHaveBeenCalled();
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(fixture.fetchBoundary).toHaveBeenCalledTimes(2);
  } finally {
    vi.useRealTimers();
    await close();
  }
});
