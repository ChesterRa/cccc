// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, expect, it, vi } from "vite-plus/test";
import { useGroupStore, useUIStore } from "../stores";
import { getChatSession } from "../stores/useUIStore";
import type { LedgerEvent } from "../types";
import { useSSE } from "./useSSE";
import { fetchLedgerBoundary } from "../services/api";

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
const fixture = vi.hoisted(() => ({
  sources: [] as FixtureSource[],
  resumed: false,
  missing: [] as LedgerEvent[],
  initial: [] as LedgerEvent[],
  searchGate: null as Promise<void> | null,
  tailGate: null as Promise<void> | null,
  boundaryFailed: false,
}));
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
        retryPendingSocket: vi.fn().mockReturnValue(false),
      },
    };
    fixture.sources.push(source);
    return source;
  },
}));
function message(id: string, by = "worker"): LedgerEvent {
  return {
    v: 1,
    id,
    ts: "2026-10-31T12:00:00Z",
    kind: "chat.message",
    group_id: "g",
    scope_key: "",
    by,
    data: { text: id, message_mode: "mail", to: ["recipient"] },
  } as LedgerEvent;
}
vi.mock("../services/api", () => ({
  withAuthToken: (path: string) => path,
  fetchGroup: vi.fn().mockResolvedValue({ ok: false }),
  fetchContext: vi.fn().mockResolvedValue({ ok: false }),
  invalidateContextRead: vi.fn(),
  fetchLedgerTail: vi.fn().mockImplementation(async () => {
    if (fixture.tailGate) await fixture.tailGate;
    return {
      ok: true,
      result: { events: fixture.initial, has_more: false, count: fixture.initial.length },
    };
  }),
  fetchLedgerBoundary: vi
    .fn()
    .mockImplementation(async () =>
      fixture.boundaryFailed
        ? { ok: false, error: { code: "NETWORK_ERROR", message: "offline" } }
        : {
            ok: true,
            result: {
              events: [fixture.resumed ? fixture.missing.at(-1) : fixture.initial.at(-1)],
              has_more: true,
              count: 1,
            },
          },
    ),
  searchChatMessages: vi
    .fn()
    .mockImplementation(
      async (_group: string, _query: string, options: { after: string; limit: number }) => {
        if (fixture.searchGate) await fixture.searchGate;
        const offset =
          options.after === "anchor" ? 0 : Number(options.after.replace("missing-", "")) + 1;
        const events = fixture.missing.slice(offset, offset + options.limit);
        return {
          ok: true,
          result: {
            events,
            has_more: offset + events.length < fixture.missing.length,
            count: events.length,
          },
        };
      },
    ),
  fetchLedgerStatuses: vi.fn().mockResolvedValue({ ok: true, result: { statuses: {} } }),
}));

beforeEach(() => {
  fixture.sources.length = 0;
  fixture.resumed = false;
  fixture.missing = [];
  fixture.initial = [message("anchor")];
  fixture.searchGate = null;
  fixture.tailGate = null;
  fixture.boundaryFailed = false;
});

async function setup(atBottom = false) {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  const oldGroup = useGroupStore.getState();
  const oldUI = useUIStore.getState();
  const incrementActorUnread = vi.fn();
  const refreshActors = vi.fn().mockResolvedValue(undefined);
  useGroupStore.setState({
    selectedGroupId: "g",
    actors: [],
    chatByGroup: {},
    refreshActors,
    incrementActorUnread,
  });
  useUIStore.setState({
    activeTab: "chat",
    chatSessions: {},
    isSmallScreen: false,
    workspaceFileViewerGroupId: "",
  });
  const bottomRef = { current: atBottom };
  let connection: ReturnType<typeof useSSE>;
  function Probe() {
    connection = useSSE({
      activeTabRef: { current: "chat" },
      chatAtBottomRef: bottomRef,
      actorsRef: { current: [{ id: "recipient" }] as never[] },
    });
    return null;
  }
  const root = createRoot(document.createElement("div"));
  let mounted = true;
  await act(async () => root.render(<Probe />));
  await act(async () => connection!.connectStream("g"));
  const ledger = fixture.sources.find((source) => source.url.includes("/ledger/stream"))!;
  async function open() {
    await act(async () => {
      ledger.readyState = 1;
      ledger.onopen!();
      await new Promise((r) => setTimeout(r, 10));
    });
  }
  function deliver(event: LedgerEvent) {
    ledger.listeners.get("ledger")!(new MessageEvent("ledger", { data: JSON.stringify(event) }));
  }
  function count() {
    return getChatSession("g", useUIStore.getState().chatSessions).chatUnreadCount;
  }
  async function switchGroup(groupId: string) {
    await act(async () => {
      useGroupStore.setState({ selectedGroupId: groupId });
    });
    await act(async () => connection!.connectStream(groupId));
    return [...fixture.sources].reverse().find((source) => source.url.includes("/ledger/stream"))!;
  }
  async function unmount() {
    if (!mounted) return;
    mounted = false;
    await act(async () => root.unmount());
  }
  async function close() {
    await unmount();
    useGroupStore.setState(oldGroup, true);
    useUIStore.setState(oldUI, true);
  }
  return {
    open,
    deliver,
    count,
    close,
    unmount,
    bottomRef,
    incrementActorUnread,
    refreshActors,
    switchGroup,
    ledger,
  };
}

it("refreshes authoritative actor badges after an initial snapshot wins the Mail race", async () => {
  const test = await setup();
  try {
    fixture.initial.push(message("initial-mail"));
    const seen: boolean[] = [];
    test.refreshActors.mockImplementation(async () => {
      seen.push(
        !!useGroupStore
          .getState()
          .chatByGroup.g?.events.some((event) => event.id === "initial-mail"),
      );
    });
    await test.open();
    expect(seen).toEqual([true]);
    expect(test.refreshActors).toHaveBeenCalledWith("g", { includeUnread: true });
    await act(async () => {
      test.deliver(message("initial-mail"));
    });
    expect(test.incrementActorUnread).not.toHaveBeenCalled();
    expect(test.count()).toBe(0);
  } finally {
    await test.close();
  }
});

it("does not refresh the old group's actors after its initial snapshot completes late", async () => {
  const test = await setup();
  try {
    let release!: () => void;
    fixture.tailGate = new Promise((resolve) => {
      release = resolve;
    });
    await test.open();
    await test.switchGroup("other");
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 10));
    });
    expect(test.refreshActors).not.toHaveBeenCalled();
  } finally {
    await test.close();
  }
});

it("counts missed messages once even when HTTP wins against replay and trims the first messages", async () => {
  const test = await setup();
  try {
    await test.open();
    expect(test.count()).toBe(0); // Initial historical snapshot is not new.
    fixture.missing = [
      ...Array.from({ length: 805 }, (_, index) => message(`missing-${index}`)),
      message("own", "user"),
    ];
    fixture.resumed = true;
    await test.open();
    expect(test.count()).toBe(805);
    expect(useGroupStore.getState().chatByGroup.g.events).toHaveLength(800);
    await act(async () => {
      for (const event of fixture.missing) test.deliver(event);
    });
    expect(test.count()).toBe(805);
    const events = useGroupStore.getState().chatByGroup.g.events;
    expect(events).toHaveLength(800);
    expect(events.at(-1)?.id).toBe("own");
    expect(events.some((event) => event.id === "missing-0")).toBe(false);
    expect(test.incrementActorUnread).not.toHaveBeenCalled();
    await act(async () => {
      test.deliver(message("new-live"));
      test.deliver(message("new-live"));
    });
    expect(test.count()).toBe(806);
    expect(useGroupStore.getState().chatByGroup.g.events.at(-1)?.id).toBe("new-live");
  } finally {
    await test.close();
  }
});

it("counts a missed event already fetched in a sender view once, even when replay follows", async () => {
  const test = await setup();
  try {
    await test.open();
    const missed = message("missing-0");
    useGroupStore
      .getState()
      .setSenderHistory(
        {
          key: "worker",
          senders: ["worker"],
          events: [missed],
          messageRevision: 0,
          receiptRevision: 0,
        },
        "g",
      );
    fixture.missing = [missed];
    fixture.resumed = true;
    await test.open();
    expect(test.count()).toBe(1);
    await act(async () => {
      test.deliver(missed);
    });
    expect(test.count()).toBe(1);
    expect(
      useGroupStore.getState().chatByGroup.g.events.filter((event) => event.id === missed.id),
    ).toHaveLength(1);
  } finally {
    await test.close();
  }
});

it.each(["live", "catch-up"] as const)(
  "counts a new event already prefetched into the live cache once on %s delivery",
  async (delivery) => {
    const test = await setup();
    try {
      await test.open();
      const missed = message("missing-0");
      useGroupStore.getState().setEvents([message("anchor"), missed], "g");
      if (delivery === "catch-up") {
        fixture.missing = [missed];
        fixture.resumed = true;
        await test.open();
      }
      await act(async () => {
        test.deliver(missed);
        test.deliver(missed);
      });
      expect(test.count()).toBe(1);
      expect(
        useGroupStore.getState().chatByGroup.g.events.filter((event) => event.id === missed.id),
      ).toHaveLength(1);
    } finally {
      await test.close();
    }
  },
);

it("counts only the unseen events when live delivery wins the race with an HTTP catch-up", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = [message("missing-0"), message("missing-1")];
    fixture.resumed = true;
    let release!: () => void;
    fixture.searchGate = new Promise((resolve) => {
      release = resolve;
    });
    await test.open();
    await act(async () => {
      test.deliver(fixture.missing[0]);
    });
    expect(test.count()).toBe(1);
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(2);
    await act(async () => {
      fixture.missing.forEach(test.deliver);
    });
    expect(test.count()).toBe(2);
  } finally {
    await test.close();
  }
});

it("discards a catch-up from the previous visit after switching away and back", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = [message("missing-0")];
    fixture.resumed = true;
    let release!: () => void;
    fixture.searchGate = new Promise((resolve) => {
      release = resolve;
    });
    await test.open();
    await test.switchGroup("other");
    const current = await test.switchGroup("g");
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(0);
    expect(
      useGroupStore.getState().chatByGroup.g.events.some((event) => event.id === "missing-0"),
    ).toBe(false);
    await act(async () => {
      current.readyState = 1;
      current.onopen!();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(1);
    expect(
      useGroupStore.getState().chatByGroup.g.events.some((event) => event.id === "missing-0"),
    ).toBe(true);
  } finally {
    await test.close();
  }
});

it("does not apply a delayed catch-up after the chat owner unmounts", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = [message("missing-0")];
    fixture.resumed = true;
    let release!: () => void;
    fixture.searchGate = new Promise((resolve) => {
      release = resolve;
    });
    await test.open();
    await test.unmount();
    test.refreshActors.mockClear();
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(0);
    expect(
      useGroupStore.getState().chatByGroup.g.events.some((event) => event.id === "missing-0"),
    ).toBe(false);
    expect(test.refreshActors).not.toHaveBeenCalled();
  } finally {
    await test.close();
  }
});

it("does not report caught-up messages as unread if the reader is following the bottom", async () => {
  const test = await setup(true);
  try {
    await test.open();
    fixture.missing = [message("missing-0"), message("missing-1")];
    fixture.resumed = true;
    await test.open();
    expect(test.count()).toBe(0);
    test.bottomRef.current = false;
    await act(async () => {
      fixture.missing.forEach(test.deliver);
    });
    expect(test.count()).toBe(0);
  } finally {
    await test.close();
  }
});

it("coalesces a focus event immediately following a successful reconnect catch-up", async () => {
  const test = await setup();
  try {
    await test.open();
    const calls = vi.mocked(fetchLedgerBoundary).mock.calls.length;
    await act(async () => {
      window.dispatchEvent(new Event("focus"));
      await Promise.resolve();
    });
    expect(vi.mocked(fetchLedgerBoundary).mock.calls.length).toBe(calls);
    const now = Date.now();
    const clock = vi.spyOn(Date, "now").mockReturnValue(now + 600);
    try {
      await act(async () => {
        window.dispatchEvent(new Event("focus"));
        await Promise.resolve();
      });
      expect(vi.mocked(fetchLedgerBoundary).mock.calls.length).toBe(calls + 1);
    } finally {
      clock.mockRestore();
    }
  } finally {
    await test.close();
  }
});

it("clears recovered IDs on a group switch", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = [message("missing-0")];
    fixture.resumed = true;
    await test.open();
    const next = await test.switchGroup("other");
    await act(async () => {
      next.listeners.get("ledger")!(
        new MessageEvent("ledger", {
          data: JSON.stringify({ ...message("missing-0"), group_id: "other" }),
        }),
      );
    });
    expect(getChatSession("other", useUIStore.getState().chatSessions).chatUnreadCount).toBe(1);
    expect(useGroupStore.getState().chatByGroup.other.events.map((event) => event.id)).toContain(
      "missing-0",
    );
  } finally {
    await test.close();
  }
});

it("restarts an overflowed replay at a fresh subscription and catches messages arriving in the gap", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = Array.from({ length: 10001 }, (_, index) => message(`missing-${index}`));
    fixture.resumed = true;
    await test.open();
    expect(test.count()).toBe(10001);
    expect(test.ledger.close).toHaveBeenCalledOnce();
    const ledger = [...fixture.sources]
      .reverse()
      .find((source) => source.url.includes("/ledger/stream"))!;
    expect(ledger).not.toBe(test.ledger);
    // Old-socket frames are ignored after the subscription was closed.
    await act(async () => {
      test.deliver(fixture.missing[0]);
    });
    expect(test.count()).toBe(10001);
    expect(useGroupStore.getState().chatByGroup.g.events).toHaveLength(800);
    expect(useGroupStore.getState().chatByGroup.g.events.at(-1)?.id).toBe("missing-10000");
    // The new source starts at EOF. The second forward read covers a message
    // written after the first HTTP snapshot but before the new subscription.
    fixture.missing.push(message("missing-10001"));
    await act(async () => {
      ledger.onopen!();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(10002);
    expect(useGroupStore.getState().chatByGroup.g.events.at(-1)?.id).toBe("missing-10001");
    expect(fixture.sources.filter((source) => source.url.includes("/ledger/stream"))).toHaveLength(
      2,
    );
  } finally {
    await test.close();
  }
});

it("keeps the overflow boundary when the first read after the new subscription fails", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = Array.from({ length: 10001 }, (_, index) => message(`missing-${index}`));
    fixture.resumed = true;
    await test.open();
    const ledger = [...fixture.sources]
      .reverse()
      .find((source) => source.url.includes("/ledger/stream"))!;
    fixture.missing.push(message("missing-10001"));
    fixture.boundaryFailed = true;
    await act(async () => {
      ledger.onopen!();
      await new Promise((resolve) => setTimeout(resolve, 10));
    });
    expect(test.count()).toBe(10001);
    fixture.boundaryFailed = false;
    await act(async () => {
      ledger.onopen!();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(10002);
    expect(useGroupStore.getState().chatByGroup.g.events.at(-1)?.id).toBe("missing-10001");
  } finally {
    await test.close();
  }
});

it("does not apply an in-flight overflow catch-up after switching groups", async () => {
  const test = await setup();
  try {
    await test.open();
    fixture.missing = Array.from({ length: 10001 }, (_, index) => message(`missing-${index}`));
    fixture.resumed = true;
    let release!: () => void;
    fixture.searchGate = new Promise((resolve) => {
      release = resolve;
    });
    await test.open();
    const other = await test.switchGroup("other");
    const before = fixture.sources.length;
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(fixture.sources.length).toBe(before);
    expect(test.count()).toBe(0);
    expect(other.close).not.toHaveBeenCalled();
  } finally {
    await test.close();
  }
});

it("waits for visibility before restarting an overflowed subscription", async () => {
  const test = await setup();
  const previous = Object.getOwnPropertyDescriptor(document, "hidden");
  let hidden = false;
  Object.defineProperty(document, "hidden", { configurable: true, get: () => hidden });
  try {
    await test.open();
    fixture.missing = Array.from({ length: 10001 }, (_, index) => message(`missing-${index}`));
    fixture.resumed = true;
    let release!: () => void;
    fixture.searchGate = new Promise((resolve) => {
      release = resolve;
    });
    await test.open();
    hidden = true;
    await act(async () => {
      release();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.ledger.close).toHaveBeenCalledOnce();
    expect(fixture.sources.filter((source) => source.url.includes("/ledger/stream"))).toHaveLength(
      1,
    );
    expect(test.count()).toBe(10001);
    fixture.searchGate = null;
    fixture.missing.push(message("missing-10001"));
    hidden = false;
    await act(async () => {
      document.dispatchEvent(new Event("visibilitychange"));
    });
    const ledger = [...fixture.sources]
      .reverse()
      .find((source) => source.url.includes("/ledger/stream"))!;
    expect(ledger).not.toBe(test.ledger);
    await act(async () => {
      ledger.onopen!();
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(test.count()).toBe(10002);
  } finally {
    if (previous) Object.defineProperty(document, "hidden", previous);
    else Reflect.deleteProperty(document, "hidden");
    await test.close();
  }
});
