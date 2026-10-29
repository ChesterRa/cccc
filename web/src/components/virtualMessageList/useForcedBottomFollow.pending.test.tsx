// @vitest-environment happy-dom

import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { useForcedBottomFollow } from "./useForcedBottomFollow";
import { usePendingReplyFollow } from "./usePendingReplyFollow";

const request = {
  id: "question",
  kind: "chat.message",
  by: "lead",
  data: { message_mode: "request_reply", to: ["user"] },
} as LedgerEvent;

type Handles = { parent: HTMLDivElement | null; start: () => void; force: () => boolean };

function Probe({
  onReady,
  scrollBottom,
  detach,
}: {
  onReady: (handles: Handles) => void;
  scrollBottom: () => void;
  detach: () => void;
}) {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const requestTokenRef = useRef(0);
  const scrollRafRef = useRef<number | null>(null);
  const forceStickToBottomRef = useRef(false);
  const forced = useForcedBottomFollow({
    requestTokenRef,
    scrollRafRef,
    forceStickToBottomRef,
    scrollToBottom: scrollBottom,
  });
  const pending = usePendingReplyFollow({
    parentRef,
    topInset: 100,
    cancelBottomScroll: forced.cancelPendingBottomScroll,
    detach,
  });
  return (
    <div
      ref={(parent) => {
        parentRef.current = parent;
        onReady({
          parent,
          force: forced.shouldForceStickToBottom,
          start: () => {
            pending.consider(request);
            forced.scheduleForceStickToBottom(pending.revealIfNeeded);
          },
        });
      }}
    />
  );
}

describe("pending request during initial forced follow", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  });

  it("cancels the forced bottom scroll before revealing the question", async () => {
    let handles: Handles;
    const scrollBottom = vi.fn();
    const detach = vi.fn();
    await act(async () =>
      root.render(
        <Probe
          onReady={(next) => {
            handles = next;
          }}
          scrollBottom={scrollBottom}
          detach={detach}
        />,
      ),
    );
    const parent = handles!.parent!;
    Object.defineProperty(parent, "clientHeight", { value: 300 });
    Object.defineProperty(parent, "scrollHeight", { value: 1000 });
    parent.scrollTo = vi.fn();
    parent.getBoundingClientRect = () => new DOMRect(0, 0, 400, 300);
    const row = document.createElement("div");
    row.dataset.messageRow = "true";
    row.dataset.messageId = request.id;
    row.getBoundingClientRect = () => new DOMRect(0, 140, 400, 600);
    row.innerHTML = "<form data-pending-reply></form>";
    parent.append(row);
    handles!.start();
    expect(handles!.force()).toBe(true);
    vi.advanceTimersToNextFrame();
    vi.advanceTimersToNextFrame();
    expect(detach).toHaveBeenCalledOnce();
    expect(handles!.force()).toBe(false);
    expect(scrollBottom).not.toHaveBeenCalled();
    expect(parent.scrollTo).toHaveBeenCalledWith({ top: 32, behavior: "auto" });
  });
});
