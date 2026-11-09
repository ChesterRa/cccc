// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../types";
import { useReplyTargetNavigation } from "./useReplyTargetNavigation";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

const message = (id: string) => ({ id, kind: "chat.message", data: {} }) as unknown as LedgerEvent;

// Every message row receives openReplyTarget. It keeps one identity across renders and finds the
// target among the messages of the latest render.
it("keeps openReplyTarget across renders and jumps within the latest messages", async () => {
  const scrollToIndexStable = vi.fn();
  const opens: ((id: string) => void)[] = [];
  function Probe({ messages }: { messages: LedgerEvent[] }) {
    opens.push(
      useReplyTargetNavigation({
        messages,
        shouldVirtualize: true,
        missingTargetMessage: "missing",
        parentRef: { current: null },
        onScrollTopChange: () => undefined,
        getMessageRowById: () => null,
        scrollToIndexStable,
        cancelPendingBottomScroll: () => undefined,
        setAtBottom: () => undefined,
        setFollowMode: () => undefined,
      }).openReplyTarget,
    );
    return null;
  }
  const root = createRoot(document.createElement("div"));
  try {
    await act(async () => root.render(<Probe messages={[message("m1")]} />));
    await act(async () => root.render(<Probe messages={[message("m1"), message("m2")]} />));
    expect(opens[opens.length - 1]).toBe(opens[0]);
    act(() => opens[0]("m2"));
    expect(scrollToIndexStable).toHaveBeenCalledWith(1);
  } finally {
    act(() => root.unmount());
  }
});
