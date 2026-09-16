// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";
import { EventStreamSource } from "./eventStream";
import { EventStreamTransport } from "./transport";
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
