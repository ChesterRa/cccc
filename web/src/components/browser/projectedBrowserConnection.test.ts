import { describe, expect, it } from "vite-plus/test";

import { resolveBrowserObserverDisconnect } from "./projectedBrowserConnection";

describe("resolveBrowserObserverDisconnect", () => {
  it("keeps a live browser session active after viewer retries are exhausted", () => {
    const result = resolveBrowserObserverDisconnect({
      surface: { active: true, state: "ready", url: "https://example.com" },
      reconnectAttempts: 3,
      maxReconnectAttempts: 3,
      reconnectingMessage: "Reconnecting",
      closedMessage: "Closed",
    });

    expect(result.shouldReconnect).toBe(false);
    expect(result.state).toMatchObject({
      active: true,
      state: "disconnected",
      url: "https://example.com",
    });
  });

  it("only reports closed when the browser session itself is inactive", () => {
    const result = resolveBrowserObserverDisconnect({
      surface: { active: false, state: "idle" },
      reconnectAttempts: 0,
      maxReconnectAttempts: 3,
      reconnectingMessage: "Reconnecting",
      closedMessage: "Closed",
    });

    expect(result.shouldReconnect).toBe(false);
    expect(result.state).toMatchObject({ active: false, state: "closed", message: "Closed" });
  });

  it("normalizes backend state casing before classifying liveness", () => {
    const result = resolveBrowserObserverDisconnect({
      surface: { active: true, state: " READY " },
      reconnectAttempts: 0,
      maxReconnectAttempts: 3,
      reconnectingMessage: "Reconnecting",
      closedMessage: "Closed",
    });

    expect(result.shouldReconnect).toBe(true);
    expect(result.state).toMatchObject({ active: true, state: "ready" });
  });
});
