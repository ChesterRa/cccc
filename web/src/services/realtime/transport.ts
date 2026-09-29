import type { Channel, EventStreamSource } from "./eventStream";

export class EventStreamTransport {
  private sources = new Map<Channel, EventStreamSource>();
  private socket: WebSocket | null = null;
  private retry: ReturnType<typeof setTimeout> | null = null;
  // Set from a socket failure until a new socket opens: covers both the
  // backoff wait and the reconnect handshake.
  private retryAt = 0;
  private delay = 1000;
  private scheduled = false;
  private channelRetries = new Map<
    EventStreamSource,
    { timer: ReturnType<typeof setTimeout>; at: number }
  >();
  private watchdog: ReturnType<typeof setTimeout> | null = null;
  constructor(readonly url: string) {}

  add(source: EventStreamSource) {
    this.sources.get(source.channel)?.close();
    this.sources.set(source.channel, source);
    // A subscription opened while the socket recovers (e.g. a group switch)
    // must still learn about it; announce once the caller attached listeners.
    if (this.retryAt) queueMicrotask(() => this.announceRetry(source));
    this.schedule();
  }
  remove(source: EventStreamSource) {
    if (this.sources.get(source.channel) !== source) return;
    this.sources.delete(source.channel);
    const retry = this.channelRetries.get(source);
    if (retry) clearTimeout(retry.timer);
    this.channelRetries.delete(source);
    this.send({ type: "unsubscribe", channel: source.channel, id: source.id });
    // A group switch closes and replaces subscriptions in one task. Delay the
    // final transport close so that switch never creates an extra socket.
    this.schedule();
  }
  dispose() {
    if (this.retry) clearTimeout(this.retry);
    this.retry = null;
    this.retryAt = 0;
    if (this.watchdog) clearTimeout(this.watchdog);
    this.watchdog = null;
    for (const retry of this.channelRetries.values()) clearTimeout(retry.timer);
    this.channelRetries.clear();
    const socket = this.socket;
    this.socket = null;
    socket?.close();
    for (const source of this.sources.values()) source.readyState = 2;
    this.sources.clear();
  }
  private schedule() {
    if (this.scheduled) return;
    this.scheduled = true;
    queueMicrotask(() => {
      this.scheduled = false;
      this.sync();
    });
  }
  private send(value: unknown) {
    const socket = this.socket;
    if (socket?.readyState !== WebSocket.OPEN) return;
    try {
      socket.send(JSON.stringify(value));
    } catch {
      this.fail(socket);
    }
  }
  private sync() {
    if (!this.sources.size) {
      this.dispose();
      return;
    }
    if (this.retry) return;
    if (!this.socket) {
      this.connect();
      return;
    }
    if (this.socket.readyState !== WebSocket.OPEN) return;
    for (const source of this.sources.values()) {
      if (source.sent || this.channelRetries.has(source)) continue;
      source.sent = true;
      this.send({
        type: "subscribe",
        channel: source.channel,
        id: source.id,
        group_id: source.groupId,
        cursor: source.cursor,
        replay: source.everSent || source.replay,
      });
      source.everSent = true;
    }
  }
  private connect() {
    const socket = new WebSocket(this.url);
    this.socket = socket;
    this.armWatchdog(socket);
    socket.onopen = () => {
      if (this.socket !== socket) return;
      this.armWatchdog(socket);
      this.retryAt = 0;
      // Subscriptions still in their own backoff now wait only on that.
      for (const source of this.channelRetries.keys()) this.announceRetry(source);
      this.sync();
    };
    socket.onmessage = (event) => {
      if (this.socket !== socket) return;
      this.armWatchdog(socket);
      try {
        const packet = JSON.parse(String(event.data));
        if (packet.type === "fatal") {
          this.fail(socket);
          return;
        }
        const source = this.sources.get(packet.channel);
        if (!source || source.id !== packet.id) return;
        if (packet.type === "ready") {
          this.delay = 1000;
          source.retryDelay = 1000;
          source.emit("open");
        } else if (packet.type === "event" && typeof packet.message?.event === "string") {
          source.emit(packet.message.event, packet.message.data, packet.message.id || "");
        } else if (packet.type === "closed") {
          this.retrySource(source);
        }
      } catch {
        this.fail(socket);
      }
    };
    socket.onerror = () => this.fail(socket);
    socket.onclose = () => this.fail(socket);
  }
  private armWatchdog(socket: WebSocket) {
    if (this.watchdog) clearTimeout(this.watchdog);
    this.watchdog = setTimeout(() => this.fail(socket), 45000);
  }
  private retrySource(source: EventStreamSource) {
    source.readyState = 0;
    source.emit("error");
    if (this.sources.get(source.channel) !== source || this.channelRetries.has(source)) return;
    source.sent = false;
    this.channelRetries.set(source, {
      timer: setTimeout(() => {
        this.channelRetries.delete(source);
        this.schedule();
      }, source.retryDelay),
      at: Date.now() + source.retryDelay,
    });
    this.announceRetry(source);
    source.retryDelay = Math.min(source.retryDelay * 2, 30000);
  }
  private fail(socket: WebSocket) {
    if (this.socket !== socket) return;
    this.socket = null;
    if (this.watchdog) clearTimeout(this.watchdog);
    this.watchdog = null;
    socket.close();
    for (const source of [...this.sources.values()]) {
      if (this.sources.get(source.channel) !== source) continue;
      source.sent = false;
      source.readyState = 0;
      source.emit("error");
    }
    if (!this.sources.size || this.retry) return;
    this.retryAt = Date.now() + this.delay;
    this.retry = setTimeout(() => {
      this.retry = null;
      this.sync();
    }, this.delay);
    for (const source of this.sources.values()) this.announceRetry(source);
    this.delay = Math.min(this.delay * 2, 30000);
  }
  /**
   * Tells a subscription why it is down and when it can resubscribe. While
   * the socket recovers that is the reason, and the subscription also waits
   * out its own later backoff; afterwards only its own backoff remains.
   */
  private announceRetry(source: EventStreamSource) {
    if (this.sources.get(source.channel) !== source) return;
    const channelAt = this.channelRetries.get(source)?.at ?? 0;
    const at = this.retryAt ? Math.max(this.retryAt, channelAt) : channelAt;
    if (!at) return;
    source.emit("retry", {
      delay_ms: Math.max(0, at - Date.now()),
      cause: this.retryAt ? "socket" : "channel",
    });
  }
}
