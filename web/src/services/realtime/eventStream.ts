import { EventStreamTransport } from "./transport";

export type Channel = "global" | "ledger" | "headless";
export class EventStreamSource extends EventTarget {
  onopen: ((event: Event) => void) | null = null;
  onerror: ((event: Event) => void) | null = null;
  readyState = 0;
  cursor = "";
  sent = false;
  everSent = false;
  retryDelay = 1000;
  constructor(
    readonly transport: EventStreamTransport,
    readonly id: number,
    readonly channel: Channel,
    readonly groupId?: string,
    readonly replay = true,
  ) {
    super();
  }
  close() {
    if (this.readyState === 2) return;
    this.readyState = 2;
    this.transport.remove(this);
  }
  emit(type: string, data?: unknown, id = "") {
    if (this.readyState === 2) return;
    if (id) this.cursor = id;
    const event =
      data === undefined
        ? new Event(type)
        : new MessageEvent(type, { data: JSON.stringify(data), lastEventId: id });
    if (type === "open") {
      this.readyState = 1;
      this.onopen?.(event);
    }
    if (type === "error") this.onerror?.(event);
    this.dispatchEvent(event);
  }
}

let transport: EventStreamTransport | null = null;
let sequence = 0;
/** Logical event subscriptions share one socket per page; no HTTP SSE fallback. */
export function openEventStream(path: string): EventStreamSource {
  const url = new URL(path, window.location.href);
  const endpoint = new URL("/api/v1/events/ws", url.origin);
  endpoint.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  const frame = url.searchParams.get("connect_frame");
  if (frame) endpoint.searchParams.set("connect_frame", frame);
  const match = url.pathname.match(/^\/api\/v1\/groups\/([^/]+)\/(ledger|headless|codex)\/stream$/);
  if (url.pathname !== "/api/v1/events/stream" && !match)
    throw new Error("Unsupported event stream");
  if (!transport || transport.url !== endpoint.href) {
    transport?.dispose();
    transport = new EventStreamTransport(endpoint.href);
  }
  const channel: Channel = match ? (match[2] === "ledger" ? "ledger" : "headless") : "global";
  const source = new EventStreamSource(
    transport,
    ++sequence,
    channel,
    match ? decodeURIComponent(match[1]) : undefined,
    url.searchParams.get("replay") !== "false",
  );
  transport.add(source);
  return source;
}

if (import.meta.hot) import.meta.hot.dispose(() => transport?.dispose());
