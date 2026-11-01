// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vite-plus/test";

import {
  shouldAcceptLateScrollRestoration,
  useInitialMessageScroll,
} from "./useInitialMessageScroll";

describe("shouldAcceptLateScrollRestoration", () => {
  it("accepts a late anchor only when the view initially had no restoration target", () => {
    expect(
      shouldAcceptLateScrollRestoration({
        previousRequestKey: "bottom",
        nextRequestKey: "anchor:event-1:20",
        reentryDeadline: 2_000,
        now: 1_000,
      }),
    ).toBe(true);
  });

  it("rejects restoration snapshots produced by an in-progress anchor restoration", () => {
    expect(
      shouldAcceptLateScrollRestoration({
        previousRequestKey: "anchor:event-1:20",
        nextRequestKey: "anchor:event-2:40",
        reentryDeadline: 2_000,
        now: 1_000,
      }),
    ).toBe(false);
  });

  it("takes the departing view's fresher place while the reader has not acted here", () => {
    const late = {
      previousRequestKey: "anchor:event-1:20",
      nextRequestKey: "anchor:event-2:40",
      reentryDeadline: 2_000,
      now: 1_000,
    };
    expect(shouldAcceptLateScrollRestoration({ ...late, engaged: false })).toBe(true);
    expect(shouldAcceptLateScrollRestoration({ ...late, engaged: true })).toBe(false);
    expect(
      shouldAcceptLateScrollRestoration({ ...late, nextRequestKey: "bottom", engaged: false }),
    ).toBe(false);
    expect(shouldAcceptLateScrollRestoration({ ...late, now: 2_001, engaged: false })).toBe(false);
  });

  it("lets nothing late move a view the reader has acted in, even one that opened at the bottom", () => {
    const late = {
      previousRequestKey: "bottom",
      nextRequestKey: "anchor:event-1:20",
      reentryDeadline: 2_000,
      now: 1_000,
    };
    expect(shouldAcceptLateScrollRestoration({ ...late, engaged: false })).toBe(true);
    expect(shouldAcceptLateScrollRestoration({ ...late, engaged: true })).toBe(false);
  });

  it("rejects late anchors after the handoff window", () => {
    expect(
      shouldAcceptLateScrollRestoration({
        previousRequestKey: "bottom",
        nextRequestKey: "anchor:event-1:20",
        reentryDeadline: 2_000,
        now: 2_001,
      }),
    ).toBe(false);
  });
});

describe("useInitialMessageScroll", () => {
  async function open(engaged: () => boolean) {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const frames: Array<() => void> = [];
    const begin = vi.fn(() => true);
    const options = {
      messages: [{ id: "m1", kind: "chat.message", by: "agent", data: { text: "m1" } }],
      didInitialScrollRef: { current: false },
      requestRef: { current: "" },
      reentryDeadlineRef: { current: Date.now() + 1_500 },
      anchorId: "m1",
      anchorOffsetPx: 40,
      shouldVirtualize: false,
      scheduleScroll: (action: () => void) => frames.push(action),
      scrollToIndex: vi.fn(),
      scrollToMessageAnchor: vi.fn(() => true),
      beginAnchorRestoration: begin,
      setAtBottom: vi.fn(),
      setFollowMode: vi.fn(),
      scheduleForceStickToBottom: vi.fn(),
      isEngaged: engaged,
    };
    function Harness() {
      useInitialMessageScroll(options);
      return null;
    }
    const root = createRoot(document.createElement("div"));
    await act(async () => root.render(createElement(Harness)));
    for (const frame of frames.splice(0)) frame();
    await act(async () => root.unmount());
    return { begin, setAtBottom: options.setAtBottom };
  }

  it("opens on the saved place a frame later", async () => {
    const { begin } = await open(() => false);
    expect(begin).toHaveBeenCalledExactlyOnceWith({ anchorId: "m1", offsetPx: 40 });
  });

  it("leaves the view alone when the reader acts before that frame", async () => {
    const { begin, setAtBottom } = await open(() => true);
    expect(begin).not.toHaveBeenCalled();
    expect(setAtBottom).not.toHaveBeenCalled();
  });
});
