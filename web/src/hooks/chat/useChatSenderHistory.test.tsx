// @vitest-environment happy-dom
import { StrictMode, act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import * as api from "../../services/api";
import { fetchLedgerStatuses, searchChatMessages } from "../../services/api";
import type { LedgerEvent } from "../../types";
import { useGroupStore } from "../../stores";
import { EMPTY_CHAT_BUCKET } from "../../stores/groupStoreCore";
import { groupViewCache, loadGroupInFlight } from "../../stores/groupStoreCore";
import { reconcileLedgerTail } from "../sse/reconcileLedgerTail";
import { useChatSenderHistory } from "./useChatSenderHistory";

vi.mock("../../services/api", () => ({
  searchChatMessages: vi.fn(),
  fetchLedgerStatuses: vi.fn(),
  fetchLedgerTail: vi.fn(),
  fetchLedgerBoundary: vi.fn(),
  fetchGroup: vi.fn(),
  fetchActors: vi.fn(),
  fetchContext: vi.fn(),
  fetchSettings: vi.fn(),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
let root: Root;
let host: HTMLDivElement;
let history: ReturnType<typeof useChatSenderHistory>;
const message = (id: string, by = "leader", text = id): LedgerEvent => ({
  id,
  by,
  kind: "chat.message",
  group_id: "a",
  ts: `2026-10-15T00:00:${id.padStart(2, "0")}Z`,
  data: { text },
});
const page = (events: LedgerEvent[], has_more = false) => ({
  ok: true as const,
  result: { events, has_more, count: events.length },
});
function Harness({
  group = "a",
  senders = ["leader"],
}: {
  group?: string;
  senders?: string[];
  live?: LedgerEvent[];
}) {
  history = useChatSenderHistory(group, senders);
  return null;
}
async function render(props: Parameters<typeof Harness>[0] = {}, strict = false) {
  await act(async () => {
    if (props.live) useGroupStore.getState().setEvents(props.live, props.group ?? "a");
    root.render(
      strict ? (
        <StrictMode>
          <Harness {...props} />
        </StrictMode>
      ) : (
        <Harness {...props} />
      ),
    );
  });
}
beforeEach(() => {
  vi.resetAllMocks();
  groupViewCache.clear();
  loadGroupInFlight.clear();
  useGroupStore.setState({ selectedGroupId: "a", chatByGroup: { a: { ...EMPTY_CHAT_BUCKET } } });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  vi.mocked(searchChatMessages).mockResolvedValue(page([]));
  vi.mocked(fetchLedgerStatuses).mockResolvedValue({ ok: true, result: { statuses: {} } });
  vi.mocked(api.fetchGroup).mockImplementation(() => new Promise(() => {}));
  vi.mocked(api.fetchActors).mockResolvedValue({ ok: true, result: { actors: [] } });
  const unused = { ok: false as const, error: { code: "unused", message: "Unused test read" } };
  vi.mocked(api.fetchContext).mockResolvedValue(unused);
  vi.mocked(api.fetchSettings).mockResolvedValue(unused);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

it("reads sparse sender history beyond the loaded tail without changing the raw Group cache", async () => {
  const live = [message("59", "worker")];
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([message("40"), message("50")], true))
    .mockResolvedValueOnce(page([message("10")]));
  await render({ live });
  expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
  await act(async () => {
    await history.loadMore();
  });
  expect(searchChatMessages).toHaveBeenLastCalledWith(
    "a",
    "",
    { byAny: ["leader"], before: "40", limit: 50 },
    expect.anything(),
  );
  expect(history.events.map((event) => event.id)).toEqual(["10", "40", "50"]);
  expect(history.hasMore).toBe(false);
  expect(live.map((event) => event.id)).toEqual(["59"]);
});

it("merges arriving messages and newer receipt data without skipping unseen historical pages", async () => {
  vi.mocked(searchChatMessages).mockResolvedValue(
    page([message("40"), message("50"), message("59")], true),
  );
  await render({ live: [message("01"), message("50", "leader", "newer status"), message("59")] });
  expect(history.events.map((event) => event.id)).toEqual(["40", "50", "59"]);
  expect(history.events[1].data).toEqual({ text: "newer status" });
});

it("reads the sender union with one cursor and stable identity regardless of checkbox order", async () => {
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([message("40"), message("50", "peer")], true))
    .mockResolvedValueOnce(page([message("10", "peer")]));
  const live = [message("01"), message("50", "peer", "updated"), message("59", "other")];
  await render({ senders: ["peer", "leader"], live });
  expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
  expect(history.events[1].data).toMatchObject({ text: "updated" });
  await render({ senders: ["leader", "peer"], live });
  expect(searchChatMessages).toHaveBeenCalledTimes(1);
  await act(async () => history.loadMore());
  expect(searchChatMessages).toHaveBeenCalledTimes(2);
  expect(searchChatMessages).toHaveBeenLastCalledWith(
    "a",
    "",
    { byAny: ["leader", "peer"], before: "40", limit: 50 },
    expect.anything(),
  );
  expect(history.events.map((event) => event.id)).toEqual(["10", "40", "50"]);
});

it("keeps a single read in flight even when another history load is requested", async () => {
  let resolve!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await render();
  await act(async () => {
    await history.loadMore();
    await history.loadMore();
  });
  expect(searchChatMessages).toHaveBeenCalledTimes(1);
  await act(async () => {
    resolve(page([message("50")]));
  });
  expect(history.events[0].id).toBe("50");
});

it.each([
  { group: "b", senders: ["leader"] },
  { group: "a", senders: ["worker"] },
])("ignores a departed Group or sender response: %j", async (next) => {
  let resolve!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages)
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    )
    .mockResolvedValueOnce(page([message("59", next.senders[0])]));
  await render();
  const signal = vi.mocked(searchChatMessages).mock.calls[0][3]?.signal;
  await render(next);
  await act(async () => resolve(page([message("01")])));
  expect(signal?.aborted).toBe(true);
  expect(history.events.map((event) => event.id)).toEqual(["59"]);
});

it("settles a failed read and allows an explicit retry without an automatic retry loop", async () => {
  vi.mocked(searchChatMessages)
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce(page([message("50")]));
  await render();
  expect(history).toMatchObject({ loading: false, loaded: true, hasMore: false, error: "offline" });
  expect(searchChatMessages).toHaveBeenCalledTimes(1);
  await act(async () => {
    await history.loadMore();
  });
  expect(history).toMatchObject({ loading: false, error: "" });
  expect(history.events[0].id).toBe("50");
});

it("does not query when senders are unrestricted", async () => {
  await render({ senders: [] });
  expect(searchChatMessages).not.toHaveBeenCalled();
  expect(history).toMatchObject({ loaded: true, loading: false, hasMore: false });
});

it("survives React's strict effect setup/cleanup cycle", async () => {
  vi.mocked(searchChatMessages).mockResolvedValue(page([message("50")]));
  await render({}, true);
  expect(vi.mocked(searchChatMessages).mock.calls.at(-1)?.[3]?.signal?.aborted).toBe(false);
  expect(history.events[0].id).toBe("50");
});

it("applies read-through receipts to messages outside the raw tail and context window", async () => {
  const first = {
    ...message("10"),
    data: { text: "old mail", message_mode: "mail" as const },
    _read_status: { worker: false },
  };
  const last = { ...first, id: "20" };
  vi.mocked(searchChatMessages).mockResolvedValue(page([first, last]));
  await render();
  await act(async () => useGroupStore.getState().updateReadStatus("20", "worker", "a"));
  expect(history.events.map((event) => event._read_status)).toEqual([
    { worker: true },
    { worker: true },
  ]);
  expect(useGroupStore.getState().chatByGroup.a.events).toEqual([]);
});

it("reads authoritative status when the Mail cursor belongs to an excluded sender", async () => {
  const mail = {
    ...message("10"),
    data: { text: "private mail", message_mode: "mail" as const },
    _read_status: { worker: false },
  };
  vi.mocked(searchChatMessages).mockResolvedValue(page([mail]));
  vi.mocked(fetchLedgerStatuses).mockResolvedValue({
    ok: true,
    result: { statuses: { "10": { read_status: { worker: true } } } },
  });
  await render({ live: [message("59", "other")] });
  await act(async () => useGroupStore.getState().updateReadStatus("59", "worker", "a"));
  expect(history.events[0]._read_status).toEqual({ worker: true });
  expect(fetchLedgerStatuses).toHaveBeenCalledWith("a", ["10"], { noCache: true });
  await act(async () => useGroupStore.getState().updateReadStatus("60", "worker", "a"));
  expect(fetchLedgerStatuses).toHaveBeenCalledTimes(1);
});

it("does not infer read state or apply a departed Group's receipt response", async () => {
  const mail = {
    ...message("10"),
    data: { text: "private mail", message_mode: "mail" as const },
    _read_status: { worker: false },
  };
  vi.mocked(searchChatMessages).mockResolvedValue(page([mail]));
  vi.mocked(fetchLedgerStatuses).mockResolvedValueOnce({
    ok: true,
    result: { statuses: { "10": { read_status: { worker: false } } } },
  });
  await render();
  await act(async () => useGroupStore.getState().updateReadStatus("outside", "worker", "a"));
  expect(history.events[0]._read_status).toEqual({ worker: false });
  let resolve!: (value: Awaited<ReturnType<typeof fetchLedgerStatuses>>) => void;
  vi.mocked(fetchLedgerStatuses).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolve = done;
      }),
  );
  await act(async () => useGroupStore.getState().updateReadStatus("outside-new", "worker", "a"));
  await render({ group: "b" });
  await act(async () =>
    resolve({ ok: true, result: { statuses: { "10": { read_status: { worker: true } } } } }),
  );
  expect(history.events[0]._read_status).toEqual({ worker: false });
  expect(useGroupStore.getState().chatByGroup.a.senderHistory).toBeNull();
});

it.each(["reply", "cancel", "snapshot"])(
  "updates private request status from a daemon %s",
  async (kind) => {
    const request = {
      ...message("10"),
      data: { text: "request", message_mode: "request_reply" as const, to: ["worker"] },
      _obligation_status: {
        worker: {
          reply_requested: true,
          replied: false,
          cancelled: false,
          delivery_state: "queued",
        },
      },
    };
    vi.mocked(searchChatMessages).mockResolvedValue(page([request]));
    await render();
    await act(async () => {
      const store = useGroupStore.getState();
      if (kind === "snapshot")
        store.mergeEventStatuses(
          {
            "10": {
              obligation_status: {
                worker: {
                  reply_requested: true,
                  replied: true,
                  cancelled: false,
                  delivery_state: "confirmed",
                },
              },
            },
          },
          "a",
        );
      else
        store.updateObligationStatus(
          "10",
          kind === "cancel"
            ? { cancelled: true }
            : { reply: { ...message("11", "worker"), data: { text: "answer", reply_to: "10" } } },
          "a",
        );
    });
    expect(history.events[0]._obligation_status?.worker).toMatchObject(
      kind === "cancel" ? { cancelled: true, replied: false } : { replied: true, cancelled: false },
    );
    expect(useGroupStore.getState().chatByGroup.a.events).toEqual([]);
  },
);

it("keeps newly appended matching messages visible after a daemon clock rollback", async () => {
  const newerTime = { ...message("40"), ts: "2026-10-16T10:00:00Z" };
  const olderTime = { ...message("50"), ts: "2026-10-16T09:00:00Z" };
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) =>
    options?.after ? page([olderTime]) : page([newerTime], true),
  );
  await render({ live: [newerTime] });
  await act(async () => useGroupStore.getState().appendEvent(olderTime, "a"));
  expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
  await act(async () => history.loadMore());
  expect(vi.mocked(searchChatMessages).mock.calls.at(-1)?.[2]?.before).toBe("40");
});

it("preserves ledger order of queried pages when their timestamps run backward", async () => {
  const events = [message("40"), { ...message("50"), ts: "2026-10-14T00:00:00Z" }];
  vi.mocked(searchChatMessages).mockResolvedValue(page(events));
  await render();
  expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
});

it("retains live arrivals while an initial page is pending and releases the projection on departure", async () => {
  let resolve!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages)
    .mockImplementation(async (_group, _q, options) => page(options?.after ? [message("50")] : []))
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
  await render();
  await act(async () => useGroupStore.getState().appendEvent(message("50"), "a"));
  await act(async () => resolve(page([message("40")], true)));
  expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
  await render({ group: "b" });
  expect(useGroupStore.getState().chatByGroup.a.senderHistory).toBeNull();
  expect(history.events).toEqual([]);
});

it("keeps older replayed rows before the first page available through its backward cursor", async () => {
  let resolve!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages)
    .mockImplementation(async (_group, _q, options) => page(options?.before ? [message("10")] : []))
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
  await render();
  await act(async () => {
    useGroupStore.getState().appendEvent(message("10"), "a");
    useGroupStore.getState().appendEvent(message("50"), "a");
  });
  await act(async () => resolve(page([message("40"), message("50")], true)));
  expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
  await act(async () => history.loadMore());
  expect(history.events.map((event) => event.id)).toEqual(["10", "40", "50"]);
  expect(new Set(history.events.map((event) => event.id)).size).toBe(3);
});

it("does not import raw backfill, but includes the forward suffix after a reconnect boundary", async () => {
  const anchor = message("59", "other");
  const rollback = { ...message("60"), ts: "2026-10-14T00:00:00Z" };
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) =>
    options?.after ? page([rollback]) : page([message("40"), message("50")], true),
  );
  await render({ live: [anchor] });
  await act(async () => useGroupStore.getState().setEvents([message("01"), anchor, rollback], "a"));
  expect(history.events.map((event) => event.id)).toEqual(["40", "50", "60"]);
});

it("includes new messages when the initial raw tail hydrates after the sender query", async () => {
  const known = message("40");
  const rollback = { ...message("60"), ts: "2026-10-14T00:00:00Z" };
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) =>
    options?.after ? page([rollback]) : page([known], true),
  );
  await render();
  await act(async () => useGroupStore.getState().setEvents([message("01"), known, rollback], "a"));
  expect(history.events.map((event) => event.id)).toEqual(["40", "60"]);
});

it("does not regress receipt state when an older page response arrives late", async () => {
  const mail = {
    ...message("40"),
    data: { text: "mail", message_mode: "mail" as const },
    _read_status: { worker: false },
  };
  let resolve!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([mail], true))
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    );
  await render();
  let pending!: Promise<void>;
  await act(async () => {
    pending = history.loadMore();
  });
  await act(async () => useGroupStore.getState().updateReadStatus("40", "worker", "a"));
  await act(async () => {
    resolve(page([message("10"), mail]));
    await pending;
  });
  expect(history.events.find((event) => event.id === "40")?._read_status).toEqual({ worker: true });
});

it("reconciles a delayed loadGroup tail through the actual hydration path", async () => {
  let resolveTail!: (value: ReturnType<typeof page>) => void;
  vi.mocked(api.fetchLedgerTail).mockImplementationOnce(
    () =>
      new Promise((done) => {
        resolveTail = done;
      }),
  );
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) =>
    page(options?.after ? [message("60")] : [message("40")], !options?.after),
  );
  await render();
  await act(async () => {
    void useGroupStore.getState().loadGroup("a");
  });
  await act(async () => {
    resolveTail(page([message("40"), message("60")]));
  });
  expect(useGroupStore.getState().chatByGroup.a.events.map((event) => event.id)).toEqual([
    "40",
    "60",
  ]);
  expect(history.events.map((event) => event.id)).toEqual(["40", "60"]);
});

it("keeps selected catch-up rows even when raw-cache trimming removes every shared anchor", async () => {
  const anchor = message("anchor");
  const backlog = Array.from({ length: 801 }, (_, index) =>
    message(`backlog-${index}`, index === 0 ? "leader" : "other"),
  );
  vi.mocked(api.fetchLedgerBoundary).mockResolvedValue(page([backlog.at(-1)!]));
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) => {
    if (!options?.after) return page([anchor], true);
    if (options.byAny) return page([backlog[0]]);
    const start =
      options.after === anchor.id
        ? 0
        : backlog.findIndex((event) => event.id === options.after) + 1;
    return page(backlog.slice(start, start + 200), start + 200 < backlog.length);
  });
  await render({ live: [anchor] });
  await act(async () => {
    await reconcileLedgerTail("a", () => true, anchor.id);
  });
  expect(useGroupStore.getState().chatByGroup.a.events).toHaveLength(800);
  expect(
    useGroupStore.getState().chatByGroup.a.events.some((event) => event.id === backlog[0].id),
  ).toBe(false);
  expect(history.events.map((event) => event.id)).toEqual([anchor.id, backlog[0].id]);
});

it.each(["send", "cancel"])(
  "projects Connect %s receipts onto sender-only source rows",
  async (action) => {
    const source: LedgerEvent = {
      ...message("source", "user"),
      _connect_delivery: { state: "queued" },
      _connect_cancellation: { state: "queued" },
    };
    vi.mocked(searchChatMessages).mockResolvedValue(page([source]));
    await render({ live: [source], senders: ["user"] });
    await act(async () =>
      useGroupStore
        .getState()
        .appendEvent(
          {
            ...message("receipt"),
            kind: "chat.cross_group_receipt",
            data: {
              transport: "connect",
              action,
              status: "sent",
              source_event_id: "source",
              original_event_id: "source",
              remote_event_id: "remote",
            },
          },
          "a",
        ),
    );
    const field = action === "send" ? "_connect_delivery" : "_connect_cancellation";
    expect(useGroupStore.getState().chatByGroup.a.events[0][field]?.state).toBe("sent");
    expect(history.events[0][field]?.state).toBe("sent");
  },
);

it("refreshes newly loaded older Mail after a read watermark arrives during pagination", async () => {
  const mail = (id: string): LedgerEvent => ({
    ...message(id),
    data: { text: "Mail", message_mode: "mail" },
    _read_status: { worker: false },
  });
  let resolveOlder!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([mail("40")], true))
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolveOlder = done;
        }),
    );
  vi.mocked(fetchLedgerStatuses).mockResolvedValue({
    ok: true,
    result: { statuses: { "10": { read_status: { worker: true } } } },
  });
  await render();
  let pending!: Promise<void>;
  await act(async () => {
    pending = history.loadMore();
  });
  await act(async () => useGroupStore.getState().updateReadStatus("40", "worker", "a"));
  await act(async () => {
    resolveOlder(page([mail("10")]));
    await pending;
  });
  expect(history.events.map((event) => event._read_status)).toEqual([
    { worker: true },
    { worker: true },
  ]);
});

it.each(["during", "after"])(
  "does not treat an unanchored older replay %s the first query as newest",
  async (timing) => {
    let resolveInitial!: (value: ReturnType<typeof page>) => void;
    vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) => {
      if (options?.after) return page([]);
      if (options?.before) return page([message("10")]);
      return new Promise((done) => {
        resolveInitial = done;
      });
    });
    await render();
    if (timing === "during")
      await act(async () => useGroupStore.getState().appendEvent(message("10"), "a"));
    await act(async () => {
      resolveInitial(page([message("40"), message("50")], true));
    });
    if (timing === "after")
      await act(async () => useGroupStore.getState().appendEvent(message("10"), "a"));
    expect(history.events.map((event) => event.id)).toEqual(["40", "50"]);
    await act(async () => {
      await history.loadMore();
    });
    expect(history.events.map((event) => event.id)).toEqual(["10", "40", "50"]);
    await act(async () => useGroupStore.getState().appendEvent(message("10"), "a"));
    expect(history.events.map((event) => event.id)).toEqual(["10", "40", "50"]);
  },
);

it("coalesces live arrivals and catches a later batch after its single forward read settles", async () => {
  let resolveForward!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) => {
    if (!options?.after) return page([message("40")], true);
    if (options.after === "40")
      return new Promise((done) => {
        resolveForward = done;
      });
    return page([message("63")]);
  });
  await render();
  await act(async () => {
    useGroupStore.getState().appendEvent(message("60"), "a");
    useGroupStore.getState().appendEvent(message("61"), "a");
  });
  expect(searchChatMessages).toHaveBeenCalledTimes(2);
  await act(async () => {
    useGroupStore.getState().appendEvent(message("62", "other"), "a");
    useGroupStore.getState().appendEvent(message("63"), "a");
    await history.loadMore();
  });
  expect(searchChatMessages).toHaveBeenCalledTimes(2);
  await act(async () => {
    resolveForward(page([message("60"), message("61")]));
  });
  expect(searchChatMessages).toHaveBeenCalledTimes(3);
  expect(history.events.map((event) => event.id)).toEqual(["40", "60", "61", "63"]);
});

it("pages a large sender catch-up forward without moving the older-history cursor", async () => {
  const rows = Array.from({ length: 201 }, (_, i) => message(`new-${i}`));
  vi.mocked(searchChatMessages).mockImplementation(async (_group, _q, options) => {
    if (options?.before) return page([message("10")]);
    if (!options?.after) return page([message("40")], true);
    return options.after === "40" ? page(rows.slice(0, 200), true) : page(rows.slice(200));
  });
  await render();
  await act(async () => useGroupStore.getState().appendEvent(rows.at(-1)!, "a"));
  expect(history.events.map((event) => event.id)).toEqual(["40", ...rows.map((event) => event.id)]);
  expect(searchChatMessages).toHaveBeenNthCalledWith(
    3,
    "a",
    "",
    { after: "new-199", byAny: ["leader"], limit: 200 },
    expect.anything(),
  );
  await act(async () => {
    await history.loadMore();
  });
  expect(searchChatMessages).toHaveBeenLastCalledWith(
    "a",
    "",
    { before: "40", byAny: ["leader"], limit: 50 },
    expect.anything(),
  );
});

it("keeps older pages reachable when the first matching burst follows an empty query", async () => {
  const rows = Array.from({ length: 60 }, (_, i) => message(`new-${i}`));
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([]))
    .mockImplementation(async (_group, _q, options) =>
      options?.before ? page(rows.slice(0, 10)) : page(rows.slice(10), true),
    );
  await render();
  expect(history.hasMore).toBe(false);
  await act(async () => useGroupStore.getState().appendEvent(rows.at(-1)!, "a"));
  expect(history.events).toHaveLength(50);
  expect(history.hasMore).toBe(true);
  await act(async () => {
    await history.loadMore();
  });
  expect(history.events.map((event) => event.id)).toEqual(rows.map((event) => event.id));
});

it("leaves failed forward reads available for explicit retry without restarting older pagination", async () => {
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([message("40")], true))
    .mockRejectedValueOnce(new Error("offline"))
    .mockResolvedValueOnce(page([message("60")]));
  await render();
  await act(async () => useGroupStore.getState().appendEvent(message("60"), "a"));
  expect(history.error).toBe("offline");
  expect(searchChatMessages).toHaveBeenCalledTimes(2);
  await act(async () => {
    await history.loadMore();
  });
  expect(history.events.map((event) => event.id)).toEqual(["40", "60"]);
  expect(history.error).toBe("");
  expect(searchChatMessages).toHaveBeenLastCalledWith(
    "a",
    "",
    { byAny: ["leader"], after: "40", limit: 200 },
    expect.anything(),
  );
});

it("ignores a departed Group's delayed forward response", async () => {
  let resolveForward!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages).mockImplementation(async (group, _q, options) => {
    if (options?.after)
      return new Promise((done) => {
        resolveForward = done;
      });
    return page([message(group === "a" ? "40" : "70")]);
  });
  await render();
  await act(async () => useGroupStore.getState().appendEvent(message("60"), "a"));
  const signal = vi.mocked(searchChatMessages).mock.calls.at(-1)?.[3]?.signal;
  await render({ group: "b" });
  await act(async () => {
    resolveForward(page([message("60")]));
  });
  expect(signal?.aborted).toBe(true);
  expect(history.events.map((event) => event.id)).toEqual(["70"]);
  expect(useGroupStore.getState().chatByGroup.a.senderHistory).toBeNull();
});

it("retries only failed receipt hydration after accepting an older page", async () => {
  const mail = (id: string): LedgerEvent => ({
    ...message(id),
    data: { message_mode: "mail", text: "Mail" },
    _read_status: { worker: false },
  });
  let resolveOlder!: (value: ReturnType<typeof page>) => void;
  vi.mocked(searchChatMessages)
    .mockResolvedValueOnce(page([mail("40")], true))
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolveOlder = done;
        }),
    );
  vi.mocked(fetchLedgerStatuses)
    .mockResolvedValueOnce({ ok: false, error: { code: "offline", message: "offline" } })
    .mockResolvedValueOnce({
      ok: true,
      result: { statuses: { "10": { read_status: { worker: true } } } },
    });
  await render();
  let pending!: Promise<void>;
  await act(async () => {
    pending = history.loadMore();
  });
  await act(async () => useGroupStore.getState().updateReadStatus("40", "worker", "a"));
  await act(async () => {
    resolveOlder(page([mail("10")]));
    await pending;
  });
  expect(history.error).toBe("offline");
  expect(history.events).toHaveLength(2);
  await act(async () => {
    await history.loadMore();
  });
  expect(history.error).toBe("");
  expect(history.events[0]._read_status).toEqual({ worker: true });
  expect(searchChatMessages).toHaveBeenCalledTimes(2);
  expect(fetchLedgerStatuses).toHaveBeenCalledTimes(2);
});

it.each(["unrelated", "connect"])(
  "hydrates a pending read watermark without losing newer %s facts",
  async (kind) => {
    const mail = (id: string): LedgerEvent => ({
      ...message(id),
      data: { message_mode: "mail", text: "Mail" },
      _read_status: { worker: false },
      _connect_delivery: { state: "queued" },
    });
    let resolveOlder!: (value: ReturnType<typeof page>) => void;
    let resolveStatus!: (value: Awaited<ReturnType<typeof fetchLedgerStatuses>>) => void;
    vi.mocked(searchChatMessages)
      .mockResolvedValueOnce(page([mail("40")], true))
      .mockImplementationOnce(
        () =>
          new Promise((done) => {
            resolveOlder = done;
          }),
      );
    vi.mocked(fetchLedgerStatuses).mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolveStatus = done;
        }),
    );
    await render();
    let pending!: Promise<void>;
    await act(async () => {
      pending = history.loadMore();
    });
    await act(async () => useGroupStore.getState().updateReadStatus("40", "worker", "a"));
    await act(async () => {
      resolveOlder(page([mail("10")]));
    });
    await act(async () => {
      if (kind === "unrelated")
        useGroupStore.getState().updateObligationStatus("outside", { cancelled: true }, "a");
      else
        useGroupStore
          .getState()
          .appendEvent(
            {
              ...message("receipt"),
              kind: "chat.cross_group_receipt",
              data: {
                source_event_id: "10",
                transport: "connect",
                status: "sent",
                remote_event_id: "remote",
              },
            },
            "a",
          );
      resolveStatus({
        ok: true,
        result: {
          statuses: {
            "10": {
              read_status: { worker: true },
              connect_delivery: { state: "failed", error: "old failure" },
            },
          },
        },
      });
      await pending;
    });
    expect(history.events[0]._read_status).toEqual({ worker: true });
    expect(history.events[0]._connect_delivery?.state).toBe(kind === "connect" ? "sent" : "failed");
  },
);
