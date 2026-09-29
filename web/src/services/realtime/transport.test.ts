// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { EventStreamSource } from "./eventStream";
import { EventStreamTransport } from "./transport";
import { readRetryDetail } from "../../hooks/useSseErrorDetailText";
class Socket {
  static OPEN = 1;
  static instances: Socket[] = [];
  readyState = 0;
  sent: Record<string, unknown>[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  onmessage: ((event: MessageEvent) => void) | null = null;
  constructor(readonly url: string) {
    Socket.instances.push(this);
  }
  send(value: string) {
    this.sent.push(JSON.parse(value));
  }
  close() {
    this.readyState = 3;
  }
  open() {
    this.readyState = 1;
    this.onopen?.();
  }
  message(value: unknown) {
    this.onmessage?.(new MessageEvent("message", { data: JSON.stringify(value) }));
  }
}
let transport: EventStreamTransport;
beforeEach(() => {
  vi.useFakeTimers();
  vi.stubGlobal("WebSocket", Socket);
  Socket.instances = [];
  transport = new EventStreamTransport("ws://localhost/api/v1/events/ws?connect_frame=frame");
});
afterEach(() => {
  transport.dispose();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});
function source(
  id: number,
  channel: "global" | "ledger" | "headless",
  groupId?: string,
  replay = true,
) {
  const value = new EventStreamSource(transport, id, channel, groupId, replay);
  transport.add(value);
  return value;
}
it("multiplexes channels and switches groups without reopening or accepting stale events", async () => {
  source(1, "global");
  const ledger = source(2, "ledger", "A");
  const headless = source(3, "headless", "A");
  await Promise.resolve();
  const socket = Socket.instances[0];
  socket.open();
  expect(socket.sent.filter((p) => p.type === "subscribe").map((p) => p.channel)).toEqual([
    "global",
    "ledger",
    "headless",
  ]);
  ledger.close();
  headless.close();
  const next = source(4, "ledger", "B");
  const nextHeadless = source(5, "headless", "B");
  const received = vi.fn();
  next.addEventListener("ledger", received);
  const snapshot = vi.fn();
  nextHeadless.addEventListener("headless.snapshot", snapshot);
  await Promise.resolve();
  expect(Socket.instances).toHaveLength(1);
  socket.message({
    type: "event",
    channel: "ledger",
    id: 2,
    message: { event: "ledger", id: "old", data: {} },
  });
  expect(received).not.toHaveBeenCalled();
  socket.message({
    type: "event",
    channel: "ledger",
    id: 4,
    message: { event: "ledger", id: "new", data: { text: "B" } },
  });
  expect(received).toHaveBeenCalledOnce();
  expect(next.cursor).toBe("new");
  socket.message({
    type: "event",
    channel: "headless",
    id: 5,
    message: { event: "headless.snapshot", data: { events: [{ id: "snapshot" }] } },
  });
  expect(snapshot).toHaveBeenCalledOnce();
  expect(socket.url).toContain("connect_frame=frame");
});
it("reconnects with the ledger cursor and a fresh headless snapshot, then releases every timer", async () => {
  const ledger = source(1, "ledger", "A");
  const headless = source(2, "headless", "A", false);
  await Promise.resolve();
  const first = Socket.instances[0];
  first.open();
  first.message({
    type: "event",
    channel: "ledger",
    id: 1,
    message: { event: "ledger", id: "cursor-1", data: {} },
  });
  first.onclose?.();
  await vi.advanceTimersByTimeAsync(1000);
  const next = Socket.instances[1];
  next.open();
  expect(next.sent.find((p) => p.channel === "ledger")?.cursor).toBe("cursor-1");
  expect(next.sent.find((p) => p.channel === "headless")?.replay).toBe(true);
  first.message({
    type: "event",
    channel: "ledger",
    id: 1,
    message: { event: "ledger", id: "stale", data: {} },
  });
  expect(ledger.cursor).toBe("cursor-1");
  ledger.close();
  headless.close();
  await Promise.resolve();
  expect(next.readyState).toBe(3);
  expect(vi.getTimerCount()).toBe(0);
  const pending = source(3, "ledger", "A");
  await Promise.resolve();
  const stalled = Socket.instances[2];
  await vi.advanceTimersByTimeAsync(45000);
  expect(stalled.readyState).toBe(3);
  pending.close();
  await Promise.resolve();
  expect(vi.getTimerCount()).toBe(0);
});
it("announces each scheduled retry with its real delay and whether the socket or a subscription failed", async () => {
  const ledger = source(1, "ledger", "A");
  const global = source(2, "global");
  const retries: { channel: string; detail: ReturnType<typeof readRetryDetail> }[] = [];
  for (const value of [ledger, global])
    value.addEventListener("retry", (event) =>
      retries.push({ channel: value.channel, detail: readRetryDetail(event, 0) }),
    );
  await Promise.resolve();
  Socket.instances[0].onclose?.();
  // Every subscription on a dropped socket learns the delay actually armed,
  // before the transport doubles it for the next attempt.
  expect(retries).toEqual([
    { channel: "ledger", detail: { cause: "socket", nextRetryAt: 1000 } },
    { channel: "global", detail: { cause: "socket", nextRetryAt: 1000 } },
  ]);
  await vi.advanceTimersByTimeAsync(1000);
  Socket.instances[1].onerror?.();
  expect(retries.slice(2).map((retry) => retry.detail?.nextRetryAt)).toEqual([2000, 2000]);

  await vi.advanceTimersByTimeAsync(2000);
  const socket = Socket.instances[2];
  socket.open();
  socket.message({ type: "ready", channel: "ledger", id: 1 });
  retries.length = 0;
  // A server-closed subscription on a healthy socket is not a socket failure.
  socket.message({ type: "closed", channel: "ledger", id: 1 });
  expect(retries).toEqual([{ channel: "ledger", detail: { cause: "channel", nextRetryAt: 1000 } }]);
});
it("tells a subscription opened during socket backoff when the pending retry fires", async () => {
  const global = source(1, "global");
  const ledger = source(2, "ledger", "A");
  await Promise.resolve();
  Socket.instances[0].onclose?.();
  await vi.advanceTimersByTimeAsync(400);
  // A group switch replaces the ledger subscription while the live global
  // subscription keeps the shared retry timer armed.
  ledger.close();
  const next = source(3, "ledger", "B");
  const delays: unknown[] = [];
  next.addEventListener("retry", (event) =>
    delays.push(JSON.parse(String((event as MessageEvent).data))),
  );
  await Promise.resolve();
  expect(delays).toEqual([{ delay_ms: 600, cause: "socket" }]);
  expect(global.readyState).toBe(0);

  await vi.advanceTimersByTimeAsync(600);
  Socket.instances[1].open();
  const later = source(4, "headless", "B");
  const laterRetries = vi.fn();
  later.addEventListener("retry", laterRetries);
  await Promise.resolve();
  expect(laterRetries).not.toHaveBeenCalled();
});
it("tells a subscription opened during the reconnect handshake that the socket is still recovering", async () => {
  source(1, "global");
  const ledger = source(2, "ledger", "A");
  await Promise.resolve();
  Socket.instances[0].onclose?.();
  // The backoff timer has fired and the new socket is still handshaking.
  await vi.advanceTimersByTimeAsync(1000);
  expect(Socket.instances).toHaveLength(2);
  ledger.close();
  const next = source(3, "ledger", "B");
  const retries: unknown[] = [];
  next.addEventListener("retry", (event) =>
    retries.push(JSON.parse(String((event as MessageEvent).data))),
  );
  await Promise.resolve();
  expect(retries).toEqual([{ delay_ms: 0, cause: "socket" }]);
});

it("keeps a subscription's own backoff when the socket drops and recovers first", async () => {
  source(1, "global");
  const ledger = source(2, "ledger", "A");
  const retries: unknown[] = [];
  ledger.addEventListener("retry", (event) =>
    retries.push(JSON.parse(String((event as MessageEvent).data))),
  );
  await Promise.resolve();
  const first = Socket.instances[0];
  first.open();
  first.message({ type: "ready", channel: "global", id: 1 });
  first.message({ type: "ready", channel: "ledger", id: 2 });
  // The server closes the ledger subscription twice: its own backoff grows
  // to 2s while the shared socket backoff stays at 1s.
  first.message({ type: "closed", channel: "ledger", id: 2 });
  await vi.advanceTimersByTimeAsync(1000);
  first.message({ type: "closed", channel: "ledger", id: 2 });
  retries.length = 0;

  first.onclose?.();
  // While the socket is down the socket is the reason, and the ledger cannot
  // resubscribe before its own later deadline.
  expect(retries).toEqual([{ delay_ms: 2000, cause: "socket" }]);

  await vi.advanceTimersByTimeAsync(1000);
  Socket.instances[1].open();
  // The socket is back; the ledger is still waiting on its own backoff.
  expect(retries.slice(1)).toEqual([{ delay_ms: 1000, cause: "channel" }]);
});
