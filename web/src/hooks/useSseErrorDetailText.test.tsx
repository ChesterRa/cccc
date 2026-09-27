// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useSseErrorDetailText } from "./useSseErrorDetailText";
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

describe("retry countdown", () => {
  it("shows the true remainder on first render, without waiting a tick", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    // 6.4s left must display 7 on the FIRST render. A hook that seeds its
    // clock before the error arrives renders one second too high here.
    useUIStore.setState({
      sseError: { endpoint: "realtime socket", nextRetryAt: Date.now() + 6400 },
    });
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:7");
  });

  it("rounds the remaining delay UP, so it never shows 6s for a 7s retry", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    // 5.4s left must still display 6: floor() would under-report and flash 0
    // a second early, telling the user the retry is imminent when it is not.
    useUIStore.setState({
      sseError: { endpoint: "realtime socket", nextRetryAt: Date.now() + 5400 },
    });
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:6");

    await act(async () => {
      vi.advanceTimersByTime(1400);
    });
    // 4.0s left -> 4. The interval fires every 250ms, so the newest sample is
    // at +1250ms (rem 4150 -> ceil 5); advancing past the next tick at +1500ms
    // is what moves the display to 4.
    await act(async () => {
      vi.advanceTimersByTime(200);
    });
    expect(detail()).toContain("retryingIn:4");
  });

  it("ticks the countdown down without remounting the hook", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    useUIStore.setState({
      sseError: { endpoint: "realtime socket", nextRetryAt: Date.now() + 10000 },
    });
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:10");

    for (const [advance, expected] of [
      [3000, 7],
      [3000, 4],
      [3000, 1],
    ] as const) {
      await act(async () => {
        vi.advanceTimersByTime(advance);
      });
      expect(detail()).toContain(`retryingIn:${expected}`);
    }
  });

  it("never displays a negative count after the deadline passes", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    useUIStore.setState({
      sseError: { endpoint: "realtime socket", nextRetryAt: Date.now() + 1000 },
    });
    await act(async () => root.render(<Probe />));
    await act(async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(detail()).toContain("retryingIn:0");
    expect(detail()).not.toContain("-");
  });

  it("resets the tick when a NEW retry deadline arrives", async () => {
    // Regression: the interval effect keys on nextRetryAt, but the interval only
    // calls setNow. If the deadline moves while the old timer is live the
    // display must still be derived from the newest deadline.
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    useUIStore.setState({
      sseError: { endpoint: "realtime socket", nextRetryAt: Date.now() + 10000 },
    });
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("retryingIn:10");

    useUIStore.setState({
      sseError: { endpoint: "realtime socket", nextRetryAt: Date.now() + 60000 },
    });
    await act(async () => {
      vi.advanceTimersByTime(1000);
    });
    expect(detail()).toContain("retryingIn:59");
  });
});

describe("diagnostics reflect the actual transport", () => {
  it("names the transport and the channels riding it, not a single channel URL", async () => {
    useUIStore.setState({
      sseError: { endpoint: "realtime socket (global, ledger, headless)", nextRetryAt: null },
    });
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("realtime socket");
    expect(detail()).toContain("ledger");
    expect(detail()).not.toContain("HTTP");
  });

  it("reports an unreachable transport distinctly from an HTTP status", async () => {
    useUIStore.setState({ sseError: { endpoint: "realtime socket", nextRetryAt: null } });
    await act(async () => root.render(<Probe />));
    expect(detail()).toContain("connectionUnreachable");
    expect(detail()).not.toContain("HTTP");
  });

  it("emits no detail text when connected", async () => {
    useUIStore.setState({ sseError: null });
    await act(async () => root.render(<Probe />));
    expect(detail()).toBe("");
  });
});
