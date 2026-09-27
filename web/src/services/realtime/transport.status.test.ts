// @vitest-environment happy-dom
import { expect, it } from "vite-plus/test";
import { EventStreamTransport } from "./transport";
import { EventStreamSource } from "./eventStream";

/**
 * Guards the fix for "diagnostics must reflect the actual transport".
 *
 * The defect: on a stream error the client issued an HTTP HEAD against one
 * channel's /stream URL and reported the resulting status. Every logical
 * subscription actually rides one shared WebSocket to /api/v1/events/ws, so
 * that probe described a request the client never makes — it could report an
 * HTTP status for a socket failure, or "unreachable" for a healthy socket.
 *
 * These assertions fail if the transport stops exposing the state the UI needs
 * in order to describe the transport it is really using.
 */
function makeSource(transport: EventStreamTransport, channel: "global" | "ledger" | "headless") {
  const source = new EventStreamSource(transport, 1, channel);
  transport.add(source);
  return source;
}

it("reports the shared socket URL, not a per-channel stream URL", () => {
  const transport = new EventStreamTransport("ws://localhost:8080/api/v1/events/ws");
  makeSource(transport, "ledger");
  const { url } = transport.status;
  expect(url).toContain("/api/v1/events/ws");
  expect(url).not.toContain("/ledger/stream");
  transport.dispose();
});

it("reports every channel riding the socket", () => {
  const transport = new EventStreamTransport("ws://localhost:8080/api/v1/events/ws");
  makeSource(transport, "ledger");
  makeSource(transport, "headless");
  makeSource(transport, "global");
  expect(transport.channels.sort()).toEqual(["global", "headless", "ledger"]);
  transport.dispose();
});

it("reports CLOSED when no socket has been established", () => {
  const transport = new EventStreamTransport("ws://localhost:8080/api/v1/events/ws");
  // No WebSocket constructed yet: a UI that read this as OPEN would claim a
  // healthy realtime connection that does not exist.
  expect(transport.status.readyState).toBe(WebSocket.CLOSED);
  transport.dispose();
});

it("drops channels from the report when they are removed", () => {
  const transport = new EventStreamTransport("ws://localhost:8080/api/v1/events/ws");
  const ledger = makeSource(transport, "ledger");
  makeSource(transport, "headless");
  expect(transport.channels).toContain("ledger");
  ledger.close();
  expect(transport.channels).not.toContain("ledger");
  transport.dispose();
});
