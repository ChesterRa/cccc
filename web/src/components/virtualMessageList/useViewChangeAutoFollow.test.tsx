// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { useViewChangeAutoFollow } from "./useViewChangeAutoFollow";

describe("useViewChangeAutoFollow", () => {
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

  it("returns to bottom follow mode when the message filter changes", async () => {
    const setAtBottom = vi.fn();
    const setFollowMode = vi.fn();
    const cancelAnchorRestoration = vi.fn();
    const scheduleForceStickToBottom = vi.fn();

    function Probe({ changeKey }: { changeKey: string }) {
      useViewChangeAutoFollow({
        changeKey,
        messageCount: 20,
        setAtBottom,
        setFollowMode,
        cancelAnchorRestoration,
        scheduleForceStickToBottom,
      });
      return null;
    }

    await act(async () => root.render(<Probe changeKey="request_reply" />));

    expect(scheduleForceStickToBottom).not.toHaveBeenCalled();

    await act(async () => root.render(<Probe changeKey="all" />));

    expect(setAtBottom).toHaveBeenCalledWith(true);
    expect(setFollowMode).toHaveBeenCalledWith("follow");
    expect(cancelAnchorRestoration).toHaveBeenCalledOnce();
    expect(scheduleForceStickToBottom).toHaveBeenCalledOnce();
    expect(cancelAnchorRestoration.mock.invocationCallOrder[0]).toBeLessThan(
      scheduleForceStickToBottom.mock.invocationCallOrder[0],
    );
  });

  it("resets an empty filtered view and follows when its first message arrives", async () => {
    const setAtBottom = vi.fn();
    const setFollowMode = vi.fn();
    const cancelAnchorRestoration = vi.fn();
    const scheduleForceStickToBottom = vi.fn();

    function Probe({ changeKey, messageCount }: { changeKey: string; messageCount: number }) {
      useViewChangeAutoFollow({
        changeKey,
        messageCount,
        setAtBottom,
        setFollowMode,
        cancelAnchorRestoration,
        scheduleForceStickToBottom,
      });
      return null;
    }

    await act(async () => root.render(<Probe changeKey="all" messageCount={20} />));
    await act(async () => root.render(<Probe changeKey="mail" messageCount={0} />));

    expect(setAtBottom).toHaveBeenLastCalledWith(true);
    expect(setFollowMode).toHaveBeenLastCalledWith("follow");
    expect(cancelAnchorRestoration).toHaveBeenCalledOnce();
    expect(scheduleForceStickToBottom).not.toHaveBeenCalled();

    await act(async () => root.render(<Probe changeKey="mail" messageCount={1} />));

    expect(scheduleForceStickToBottom).toHaveBeenCalledOnce();
  });
});
