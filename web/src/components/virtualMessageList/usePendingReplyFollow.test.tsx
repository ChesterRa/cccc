// @vitest-environment happy-dom

import { act, useRef } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type { LedgerEvent } from "../../types";
import { usePendingReplyFollow } from "./usePendingReplyFollow";

type Follow = ReturnType<typeof usePendingReplyFollow>;
const request = {
  id: "request-1",
  kind: "chat.message",
  by: "lead",
  data: { message_mode: "request_reply", to: ["user"] },
} as LedgerEvent;
const ordinary = {
  id: "message-2",
  kind: "chat.message",
  by: "lead",
  data: { message_mode: "send", to: ["user"] },
} as LedgerEvent;

function Probe({
  readOnly,
  onReady,
  cancelBottomScroll,
  detach,
}: {
  readOnly?: boolean;
  onReady: (follow: Follow, element: HTMLDivElement | null) => void;
  cancelBottomScroll: () => void;
  detach: () => void;
}) {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const follow = usePendingReplyFollow({
    parentRef,
    readOnly,
    topInset: 100,
    cancelBottomScroll,
    detach,
  });
  return (
    <div
      ref={(element) => {
        parentRef.current = element;
        onReady(follow, element);
      }}
    />
  );
}

describe("usePendingReplyFollow", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  let follow: Follow;
  let element: HTMLDivElement;
  let row: HTMLDivElement;
  const cancelBottomScroll = vi.fn();
  const detach = vi.fn();
  const scrollTo = vi.fn();

  async function mount(readOnly = false) {
    await act(async () =>
      root.render(
        <Probe
          readOnly={readOnly}
          cancelBottomScroll={cancelBottomScroll}
          detach={detach}
          onReady={(next, parent) => {
            follow = next;
            if (parent) element = parent;
          }}
        />,
      ),
    );
    element.scrollTo = scrollTo;
    element.scrollTop = 400;
    Object.defineProperty(element, "clientHeight", { configurable: true, value: 300 });
    Object.defineProperty(element, "scrollHeight", { configurable: true, value: 1000 });
    element.getBoundingClientRect = () => new DOMRect(0, 0, 400, 300);
    row = document.createElement("div");
    row.dataset.messageRow = "true";
    row.dataset.messageId = request.id;
    row.getBoundingClientRect = () => new DOMRect(0, 40, 400, 500);
    element.append(row);
  }

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    vi.clearAllMocks();
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  it("keeps normal follow while status loads, then reveals a tall form only once", async () => {
    await mount();
    follow.consider(request);
    expect(follow.revealIfNeeded()).toBe(false);
    expect(detach).not.toHaveBeenCalled();
    const form = document.createElement("form");
    form.dataset.pendingReply = "";
    row.append(form);
    expect(follow.revealIfNeeded()).toBe(true);
    expect(cancelBottomScroll).toHaveBeenCalledOnce();
    expect(detach).toHaveBeenCalledOnce();
    expect(scrollTo).toHaveBeenCalledWith({ top: 332, behavior: "auto" });
    expect(follow.revealIfNeeded()).toBe(false);
  });

  it("stays at the bottom for a form that fits and reveals it if later content grows", async () => {
    await mount();
    row.getBoundingClientRect = () => new DOMRect(0, 408, 400, 100);
    row.innerHTML = "<form data-pending-reply></form>";
    follow.consider(request);
    expect(follow.revealIfNeeded()).toBe(false);
    row.getBoundingClientRect = () => new DOMRect(0, 40, 400, 500);
    expect(follow.revealIfNeeded()).toBe(true);
    expect(detach).toHaveBeenCalledOnce();
    expect(follow.revealIfNeeded()).toBe(false);
  });

  it("does not reveal after a user interaction cancels a previously fitting form", async () => {
    await mount();
    row.getBoundingClientRect = () => new DOMRect(0, 408, 400, 100);
    row.innerHTML = "<form data-pending-reply></form>";
    follow.consider(request);
    expect(follow.revealIfNeeded()).toBe(false);
    follow.cancel();
    row.getBoundingClientRect = () => new DOMRect(0, 40, 400, 500);
    expect(follow.revealIfNeeded()).toBe(false);
    expect(detach).not.toHaveBeenCalled();
  });

  it("discards the candidate on user action or a newer ordinary message", async () => {
    await mount();
    row.innerHTML = "<form data-pending-reply></form>";
    follow.consider(request);
    follow.cancel();
    expect(follow.revealIfNeeded()).toBe(false);
    follow.consider(request);
    follow.consider(ordinary);
    expect(follow.revealIfNeeded()).toBe(false);
    expect(detach).not.toHaveBeenCalled();
  });

  it("does not alter follow in read-only mode", async () => {
    await mount(true);
    row.innerHTML = "<form data-pending-reply></form>";
    follow.consider(request);
    expect(follow.revealIfNeeded()).toBe(false);
    expect(detach).not.toHaveBeenCalled();
  });
});
