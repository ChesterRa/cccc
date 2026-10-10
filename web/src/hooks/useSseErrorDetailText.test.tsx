// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { readRetryDetail, useSseErrorDetailText } from "./useSseErrorDetailText";
import { useUIStore } from "../stores/useUIStore";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, opts?: Record<string, unknown>) =>
      opts && "seconds" in opts ? `${key}:${String(opts.seconds)}` : key,
  }),
}));

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

function Probe() {
  return <span data-detail>{useSseErrorDetailText()}</span>;
}

function detail(): string {
  return host.querySelector("[data-detail]")?.textContent ?? "";
}

beforeEach(() => {
  Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
  useUIStore.setState({ sseError: null });
  vi.useRealTimers();
});

function retryIn(ms: number, cause: "socket" | "channel" = "socket") {
  useUIStore.setState({ sseError: { cause, nextRetryAt: Date.now() + ms } });
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
});

describe("retry countdown", () => {
  it("shows the true remainder on first render, without waiting a tick", async () => {
    // 6.4s left must display 7 on the FIRST render. A hook that seeds its
    // clock before the error arrives renders one second too high here.
    retryIn(6400);
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:7");
  });

  it("rounds up and moves to the next second exactly when it changes", async () => {
    retryIn(5400);
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:6");
    await act(async () => vi.advanceTimersByTime(399));
    expect(detail()).toContain("retryingIn:6");
    await act(async () => vi.advanceTimersByTime(1));
    expect(detail()).toContain("retryingIn:5");
    await act(async () => vi.advanceTimersByTime(1000));
    expect(detail()).toContain("retryingIn:4");
  });

  it("ticks the countdown down without remounting the hook", async () => {
    retryIn(10000);
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:10");
    for (const expected of [7, 4, 1]) {
      await act(async () => vi.advanceTimersByTime(3000));
      expect(detail()).toContain(`retryingIn:${expected}`);
    }
  });

  it("says it is retrying at the deadline and stops waking the component", async () => {
    // The transport may sit in a new connection attempt for up to its 45s
    // watchdog after the deadline; "retrying in 0s" would be stale for all of
    // it, and a still-running timer would keep re-rendering the header.
    retryIn(1000);
    await act(async () => root.render(<Probe />));
    await act(async () => vi.advanceTimersByTime(1000));
    expect(detail()).toContain("retryingNow");
    expect(detail()).not.toContain("retryingIn");
    expect(vi.getTimerCount()).toBe(0);
  });

  it("follows a NEW retry deadline", async () => {
    retryIn(10000);
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:10");
    await act(async () => retryIn(60000));
    await act(async () => vi.advanceTimersByTime(1000));
    expect(detail()).toContain("retryingIn:59");
  });
});

describe("diagnostics reflect the actual transport", () => {
  it("reports a dropped socket as an unreachable realtime connection", async () => {
    retryIn(7000, "socket");
    await act(async () => root.render(<Probe />));
    expect(detail()).toBe("realtimeSocketUnreachable · retryingIn:7");
  });

  it("reports a server-closed subscription without calling the socket unreachable", async () => {
    retryIn(7000, "channel");
    await act(async () => root.render(<Probe />));
    expect(detail()).toBe("realtimeSubscriptionClosed · retryingIn:7");
  });

  it("shows a retry already under way (reconnect handshake) as retrying now", async () => {
    const event = new MessageEvent("retry", {
      data: JSON.stringify({ delay_ms: 0, cause: "socket" }),
    });
    useUIStore.setState({ sseError: readRetryDetail(event) });
    await act(async () => root.render(<Probe />));
    expect(detail()).toBe("realtimeSocketUnreachable · retryingNow");
  });

  it("emits no detail text when connected", async () => {
    useUIStore.setState({ sseError: null });
    await act(async () => root.render(<Probe />));
    expect(detail()).toBe("");
  });
});
