import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vite-plus/test";

const script = readFileSync(new URL("../../../public/push/sw.js", import.meta.url), "utf8");

type WorkerEvent = {
  data?: { json(): unknown };
  notification?: { title?: string; body?: string; tag?: string; data: unknown; close?(): void };
  waitUntil(promise: Promise<unknown>): void;
};

function worker() {
  const handlers = new Map<string, (event: WorkerEvent) => void>();
  const showNotification = vi.fn().mockResolvedValue(undefined);
  const matchAll = vi.fn().mockResolvedValue([]);
  const openWindow = vi.fn().mockResolvedValue(undefined);
  class FakeChannel {
    port1 = { onmessage: (_event: { data: unknown }) => {}, close: () => {} };
    port2 = {
      postMessage: (data: unknown) => queueMicrotask(() => this.port1.onmessage({ data })),
    };
  }
  runInNewContext(script, {
    self: {
      location: { origin: "https://cccc.example" },
      registration: { showNotification },
      clients: { matchAll, openWindow },
      addEventListener: (name: string, handler: (event: WorkerEvent) => void) => {
        handlers.set(name, handler);
      },
    },
    URL,
    MessageChannel: FakeChannel,
    setTimeout,
    clearTimeout,
    Date,
  });
  return { handlers, showNotification, matchAll, openWindow };
}

async function push(handler: (event: WorkerEvent) => void, payload: unknown) {
  let completion: Promise<unknown> | undefined;
  handler({
    data: { json: () => payload },
    waitUntil: (promise: Promise<unknown>) => {
      completion = promise;
    },
  });
  expect(completion).toBeDefined();
  await completion;
}

async function click(handler: (event: WorkerEvent) => void, target: unknown) {
  let completion: Promise<unknown> | undefined;
  const close = vi.fn();
  handler({
    notification: { data: target, close },
    waitUntil: (promise: Promise<unknown>) => {
      completion = promise;
    },
  });
  expect(completion).toBeDefined();
  await completion;
  expect(close).toHaveBeenCalledOnce();
}

describe("push-only service worker", () => {
  it("shows the declarative payload without installing a fetch handler", async () => {
    const w = worker();
    expect([...w.handlers.keys()]).toEqual(["push", "notificationclick"]);
    await push(w.handlers.get("push")!, {
      web_push: 8030,
      notification: {
        title: "Project · Agent",
        body: "Which option?",
        navigate: "https://outside.example/redirect",
        tag: "project-event",
        data: { v: 1, kind: "request", group_id: "g-1", event_id: "event-1" },
      },
    });
    expect(w.showNotification).toHaveBeenCalledExactlyOnceWith(
      "Project · Agent",
      expect.objectContaining({
        body: "Which option?",
        tag: "project-event",
        data: { group_id: "g-1", event_id: "event-1" },
      }),
    );
  });

  it("still displays a generic notification for malformed push data", async () => {
    const w = worker();
    let completion: Promise<unknown> | undefined;
    w.handlers.get("push")!({
      data: {
        json: () => {
          throw new Error("bad JSON");
        },
      },
      waitUntil: (promise: Promise<unknown>) => {
        completion = promise;
      },
    });
    await completion;
    expect(w.showNotification).toHaveBeenCalledExactlyOnceWith(
      "CCCC",
      expect.objectContaining({ data: null }),
    );
  });

  it("displays the same payload when it has no declarative marker", async () => {
    const w = worker();
    await push(w.handlers.get("push")!, {
      notification: {
        title: "Agent asks",
        body: "Choose",
        tag: "g-1:event-1",
        data: { v: 1, group_id: "g-1", event_id: "event-1" },
      },
    });
    expect(w.showNotification).toHaveBeenCalledExactlyOnceWith(
      "Agent asks",
      expect.objectContaining({
        body: "Choose",
        tag: "g-1:event-1",
        data: { group_id: "g-1", event_id: "event-1" },
      }),
    );
  });

  it("sends a click to an existing workbench and focuses it", async () => {
    const w = worker();
    const focus = vi.fn().mockResolvedValue(undefined);
    const postMessage = vi.fn((_: unknown, ports: { postMessage(data: unknown): void }[]) => {
      ports[0].postMessage({ handled: true });
    });
    w.matchAll.mockResolvedValue([{ url: "https://cccc.example/ui/", focus, postMessage }]);
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "event-1" });
    expect(postMessage).toHaveBeenCalledOnce();
    expect(postMessage.mock.calls[0][0]).toEqual(
      expect.objectContaining({ type: "cccc:push:open", group_id: "g-1", event_id: "event-1" }),
    );
    expect(focus).toHaveBeenCalledOnce();
    expect(focus.mock.invocationCallOrder[0]).toBeLessThan(postMessage.mock.invocationCallOrder[0]);
    expect(w.openWindow).not.toHaveBeenCalled();
  });

  it("does not replace an existing page when focus resolves after the timeout", async () => {
    vi.useFakeTimers();
    try {
      const w = worker();
      const focus = vi.fn(() => new Promise((resolve) => setTimeout(() => resolve({}), 250)));
      const postMessage = vi.fn();
      w.matchAll.mockResolvedValue([
        { url: "https://cccc.example/ui/", focus, postMessage },
        { url: "https://cccc.example/ui/other", focus, postMessage },
      ]);
      const completion = click(w.handlers.get("notificationclick")!, {
        group_id: "g-1",
        event_id: "event-1",
      });
      await vi.advanceTimersByTimeAsync(651);
      await completion;
      expect(focus).toHaveBeenCalledOnce();
      expect(postMessage).toHaveBeenCalledOnce();
      expect(w.openWindow).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not replace a focused page that cannot acknowledge the click", async () => {
    vi.useFakeTimers();
    try {
      const w = worker();
      const focus = vi.fn().mockResolvedValue({});
      const postMessage = vi.fn();
      w.matchAll.mockResolvedValue([{ url: "https://cccc.example/ui/", focus, postMessage }]);
      const completion = click(w.handlers.get("notificationclick")!, {
        group_id: "g-1",
        event_id: "event-1",
      });
      await vi.advanceTimersByTimeAsync(651);
      await completion;
      expect(focus).toHaveBeenCalledOnce();
      expect(postMessage).toHaveBeenCalledOnce();
      expect(w.openWindow).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not open another window if the accepted page cannot be focused", async () => {
    const w = worker();
    const focus = vi.fn().mockRejectedValue(new Error("focus denied"));
    const postMessage = vi.fn((_: unknown, ports: { postMessage(data: unknown): void }[]) => {
      ports[0].postMessage({ handled: true });
    });
    w.matchAll.mockResolvedValue([{ url: "https://cccc.example/ui/", focus, postMessage }]);
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "event-1" });
    expect(focus).toHaveBeenCalledOnce();
    expect(w.openWindow).not.toHaveBeenCalled();
  });

  it("does not replace an existing page if focus and acknowledgement fail", async () => {
    vi.useFakeTimers();
    try {
      const w = worker();
      const focus = vi.fn().mockRejectedValue(new Error("focus denied"));
      w.matchAll.mockResolvedValue([
        { url: "https://cccc.example/ui/", focus, postMessage: vi.fn() },
      ]);
      const completion = click(w.handlers.get("notificationclick")!, {
        group_id: "g-1",
        event_id: "event-1",
      });
      await vi.advanceTimersByTimeAsync(651);
      await completion;
      expect(w.openWindow).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("opens a same-origin message link when no workbench handles the click", async () => {
    const w = worker();
    w.matchAll.mockResolvedValue([{ url: "https://outside.example/ui/" }]);
    await click(w.handlers.get("notificationclick")!, { group_id: "g/1", event_id: "event?1" });
    expect(w.openWindow).toHaveBeenCalledExactlyOnceWith(
      "https://cccc.example/ui/?group=g%2F1&event=event%3F1",
    );
  });

  it("does not assume there are no existing pages when the client lookup times out", async () => {
    vi.useFakeTimers();
    try {
      const w = worker();
      w.matchAll.mockReturnValue(new Promise(() => {}));
      const completion = click(w.handlers.get("notificationclick")!, {
        group_id: "g-1",
        event_id: "event-1",
      });
      await vi.advanceTimersByTimeAsync(651);
      await completion;
      expect(w.openWindow).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("passes over pages that are not the workbench for one that is", async () => {
    const w = worker();
    const elsewhere = vi.fn();
    const focus = vi.fn().mockResolvedValue(undefined);
    const postMessage = vi.fn((_: unknown, ports: { postMessage(data: unknown): void }[]) => {
      ports[0].postMessage({ handled: true });
    });
    w.matchAll.mockResolvedValue([
      { url: "https://cccc.example/ui/capabilities?group_id=g-1", focus: elsewhere },
      { url: "https://cccc.example/ui/capabilities/", focus: elsewhere },
      { url: "https://cccc.example/ui/connect?frame=1", focus: elsewhere },
      { url: "https://cccc.example/ui/push/", focus: elsewhere },
      { url: "https://cccc.example/ui/?group=g-2", focus, postMessage },
    ]);
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "event-1" });
    expect(elsewhere).not.toHaveBeenCalled();
    expect(focus).toHaveBeenCalledOnce();
    expect(postMessage).toHaveBeenCalledOnce();
    expect(w.openWindow).not.toHaveBeenCalled();
  });

  it("opens the workbench when only other CCCC pages are open", async () => {
    const w = worker();
    const elsewhere = vi.fn();
    w.matchAll.mockResolvedValue([
      { url: "https://cccc.example/ui/capabilities", focus: elsewhere },
    ]);
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "event-1" });
    expect(elsewhere).not.toHaveBeenCalled();
    expect(w.openWindow).toHaveBeenCalledExactlyOnceWith(
      "https://cccc.example/ui/?group=g-1&event=event-1",
    );
  });

  it("does not route notification clicks into an embedded frame", async () => {
    const w = worker();
    const focus = vi.fn();
    w.matchAll.mockResolvedValue([{ url: "https://cccc.example/ui/", frameType: "nested", focus }]);
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "event-1" });
    expect(focus).not.toHaveBeenCalled();
    expect(w.openWindow).toHaveBeenCalledExactlyOnceWith(
      "https://cccc.example/ui/?group=g-1&event=event-1",
    );
  });

  it("falls back to the workbench for a notification without a valid target", async () => {
    const w = worker();
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "" });
    expect(w.openWindow).toHaveBeenCalledExactlyOnceWith("https://cccc.example/ui/");
  });

  it("does not use a notification target containing a control character", async () => {
    const w = worker();
    await click(w.handlers.get("notificationclick")!, { group_id: "g-1", event_id: "event\0-1" });
    expect(w.openWindow).toHaveBeenCalledExactlyOnceWith("https://cccc.example/ui/");
  });

  it("focuses an existing page instead of replacing it for a notification without a target", async () => {
    const w = worker();
    const focus = vi.fn().mockResolvedValue({});
    w.matchAll.mockResolvedValue([{ url: "https://cccc.example/ui/", focus }]);
    await click(w.handlers.get("notificationclick")!, null);
    expect(focus).toHaveBeenCalledOnce();
    expect(w.openWindow).not.toHaveBeenCalled();
  });
});
