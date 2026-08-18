// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { ChatSendScrollRequest } from "../../utils/chatSendScrollRequest";
import { useSendScrollRequestLifecycle } from "./useSendScrollRequestLifecycle";

const request: ChatSendScrollRequest = { requestId: 7, groupId: "g1", viewKey: "g1" };

describe("useSendScrollRequestLifecycle", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("consumes an owned request and schedules one smooth forced scroll", async () => {
    const activeRequestRef = { current: null as ChatSendScrollRequest | null };
    const requestTokenRef = { current: 2 };
    const onConsumed = vi.fn();
    const setAtBottom = vi.fn();
    const setFollowMode = vi.fn();
    const scrollToBottom = vi.fn();
    const scheduleScroll = vi.fn((callback: () => void) => callback());
    const cancelPendingBottomScroll = vi.fn();

    function Probe() {
      useSendScrollRequestLifecycle({
        activeRequestRef,
        groupId: "g1",
        viewKey: "g1",
        request,
        onConsumed,
        setAtBottom,
        setFollowMode,
        requestTokenRef,
        scheduleScroll,
        scrollToBottom,
        cancelPendingBottomScroll,
      });
      return null;
    }

    await act(async () => root.render(<Probe />));

    expect(onConsumed).toHaveBeenCalledWith(7);
    expect(setAtBottom).toHaveBeenCalledWith(true);
    expect(setFollowMode).toHaveBeenCalledWith("follow");
    expect(requestTokenRef.current).toBe(3);
    expect(scrollToBottom).toHaveBeenCalledWith({
      force: true,
      requestToken: 3,
      behavior: "smooth",
      sendRequest: request,
    });
    expect(activeRequestRef.current).toBe(request);

    await act(async () => root.render(null));
    expect(activeRequestRef.current).toBeNull();
    expect(cancelPendingBottomScroll).toHaveBeenCalledOnce();
  });

  it("consumes a request owned by another view without scrolling", async () => {
    const onConsumed = vi.fn();
    const scrollToBottom = vi.fn();

    function Probe() {
      useSendScrollRequestLifecycle({
        activeRequestRef: { current: null },
        groupId: "g2",
        viewKey: "g2",
        request,
        onConsumed,
        setAtBottom: vi.fn(),
        setFollowMode: vi.fn(),
        requestTokenRef: { current: 0 },
        scheduleScroll: (callback) => callback(),
        scrollToBottom,
        cancelPendingBottomScroll: vi.fn(),
      });
      return null;
    }

    await act(async () => root.render(<Probe />));

    expect(onConsumed).toHaveBeenCalledWith(7);
    expect(scrollToBottom).not.toHaveBeenCalled();
  });
});
