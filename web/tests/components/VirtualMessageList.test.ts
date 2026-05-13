import { describe, expect, it } from "vitest";

import {
  getAutoFollowTrigger,
  getStableMessageKey,
  shouldAutoScrollToBottom,
  shouldDetachChatFollowOnScroll,
  shouldNotifyScrollChange,
  shouldRunScheduledBottomScroll,
  shouldUseVirtualizedMessageList,
} from "../../src/components/virtualMessageListHelpers";
import type { LedgerEvent } from "../../src/types";

describe("getStableMessageKey", () => {
  it("keeps a reply-slot key stable once a pending event id is known", () => {
    const base: LedgerEvent = {
      id: "local:msg-1:coder",
      kind: "chat.message",
      by: "coder",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "local:msg-1:coder",
      },
    };

    const promoted: LedgerEvent = {
      ...base,
      data: {
        ...base.data,
        stream_id: "pending:evt-1:coder",
        pending_event_id: "evt-1",
        pending_placeholder: true,
      },
    };

    const liveStream: LedgerEvent = {
      ...promoted,
      data: {
        ...promoted.data,
        stream_id: "stream-final-1",
        pending_placeholder: false,
        text: "正在输出",
      },
    };

    const completed: LedgerEvent = {
      ...liveStream,
      _streaming: false,
      data: {
        ...liveStream.data,
        text: "最终回复",
      },
    };

    expect(getStableMessageKey(base, 0)).toBe("message-event:local:msg-1:coder");
    expect(getStableMessageKey(promoted, 0)).toBe("message-event:local:msg-1:coder");
    expect(getStableMessageKey(liveStream, 0)).toBe("message-event:local:msg-1:coder");
    expect(getStableMessageKey(completed, 0)).toBe("message-event:local:msg-1:coder");
  });

  it("uses stream identity for canonical stream-backed replies even when pending_event_id exists", () => {
    const first: LedgerEvent = {
      id: "evt-1",
      kind: "chat.message",
      by: "coder",
      data: {
        text: "先看一下",
        to: ["user"],
        stream_id: "stream-1",
        pending_event_id: "evt-user-1",
      },
    };

    const second: LedgerEvent = {
      id: "evt-2",
      kind: "chat.message",
      by: "coder",
      data: {
        text: "再补一句",
        to: ["user"],
        stream_id: "stream-2",
        pending_event_id: "evt-user-1",
      },
    };

    expect(getStableMessageKey(first, 0)).toBe("stream:stream-1");
    expect(getStableMessageKey(second, 1)).toBe("stream:stream-2");
  });

  it("keeps placeholder-like same-slot messages on the pending key", () => {
    const placeholder: LedgerEvent = {
      id: "evt-placeholder",
      kind: "chat.message",
      by: "coder",
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:evt-user-1:coder",
        pending_event_id: "evt-user-1",
        pending_placeholder: true,
      },
    };

    expect(getStableMessageKey(placeholder, 0)).toBe("pending:coder:evt-user-1");
  });

  it("uses stream identity when explicit phase marks a real stream even if the id looks placeholder-like", () => {
    const commentary: LedgerEvent = {
      id: "evt-commentary",
      kind: "chat.message",
      by: "coder",
      data: {
        text: "",
        to: ["user"],
        stream_id: "pending:evt-user-2:coder:commentary",
        pending_event_id: "evt-user-2",
        pending_placeholder: false,
        stream_phase: "commentary",
      },
    };

    expect(getStableMessageKey(commentary, 0)).toBe("stream:pending:evt-user-2:coder:commentary");
  });
});

describe("shouldUseVirtualizedMessageList", () => {
  it("keeps small chat transcripts on the non-virtualized path", () => {
    expect(shouldUseVirtualizedMessageList(0)).toBe(false);
    expect(shouldUseVirtualizedMessageList(1)).toBe(false);
    expect(shouldUseVirtualizedMessageList(79)).toBe(false);
  });

  it("enables virtualization only once the transcript is large enough", () => {
    expect(shouldUseVirtualizedMessageList(80)).toBe(true);
    expect(shouldUseVirtualizedMessageList(120)).toBe(true);
  });
});

describe("getAutoFollowTrigger", () => {
  it("prefers append when a new tail item is added", () => {
    expect(
      getAutoFollowTrigger({
        previousTailSnapshot: { count: 2, tailKey: "m2" },
        nextTailSnapshot: { count: 3, tailKey: "m3" },
        previousTailMutationSnapshot: { tailKey: "m2", signature: "2|m2|assistant|t2||10|0" },
        nextTailMutationSnapshot: { tailKey: "m3", signature: "3|m3|assistant|t3||0|0" },
      }),
    ).toBe("append");
  });

  it("uses mutation when the tail item stays the same but its content changes", () => {
    expect(
      getAutoFollowTrigger({
        previousTailSnapshot: { count: 3, tailKey: "m3" },
        nextTailSnapshot: { count: 3, tailKey: "m3" },
        previousTailMutationSnapshot: { tailKey: "m3", signature: "3|m3|assistant|t3||10|0" },
        nextTailMutationSnapshot: { tailKey: "m3", signature: "3|m3|assistant|t3||40|0" },
      }),
    ).toBe("mutation");
  });

  it("returns null when neither append nor tail mutation happened", () => {
    expect(
      getAutoFollowTrigger({
        previousTailSnapshot: { count: 3, tailKey: "m3" },
        nextTailSnapshot: { count: 3, tailKey: "m3" },
        previousTailMutationSnapshot: { tailKey: "m3", signature: "3|m3|assistant|t3||40|0" },
        nextTailMutationSnapshot: { tailKey: "m3", signature: "3|m3|assistant|t3||40|0" },
      }),
    ).toBeNull();
  });
});

describe("shouldDetachChatFollowOnScroll", () => {
  it("detaches when the user scrolls upward away from bottom", () => {
    expect(
      shouldDetachChatFollowOnScroll({
        followMode: "follow",
        previousTop: 640,
        currentTop: 560,
        atBottom: false,
        isContainerResizing: false,
        topLoadThresholdPx: 80,
      }),
    ).toBe(true);
  });

  it("detaches once the viewport reaches the history-load threshold", () => {
    expect(
      shouldDetachChatFollowOnScroll({
        followMode: "follow",
        previousTop: 0,
        currentTop: 60,
        atBottom: false,
        isContainerResizing: false,
        topLoadThresholdPx: 80,
      }),
    ).toBe(true);
  });

  it("stays attached during container resize compensation", () => {
    expect(
      shouldDetachChatFollowOnScroll({
        followMode: "follow",
        previousTop: 640,
        currentTop: 560,
        atBottom: false,
        isContainerResizing: true,
        topLoadThresholdPx: 80,
      }),
    ).toBe(false);
  });

  it("does not detach when already at bottom", () => {
    expect(
      shouldDetachChatFollowOnScroll({
        followMode: "follow",
        previousTop: 640,
        currentTop: 640,
        atBottom: true,
        isContainerResizing: false,
        topLoadThresholdPx: 80,
      }),
    ).toBe(false);
  });
});

describe("shouldNotifyScrollChange", () => {
  it("notifies at bottom when parent scroll affordance state is stale", () => {
    expect(
      shouldNotifyScrollChange({
        wasAtBottom: true,
        atBottom: true,
        showScrollButton: true,
        chatUnreadCount: 1,
      }),
    ).toBe(true);
  });

  it("does not notify repeatedly when bottom state and parent affordance are already clean", () => {
    expect(
      shouldNotifyScrollChange({
        wasAtBottom: true,
        atBottom: true,
        showScrollButton: false,
        chatUnreadCount: 0,
      }),
    ).toBe(false);
  });
});

describe("shouldAutoScrollToBottom", () => {
  it("allows auto-scroll only when still following from the bottom", () => {
    expect(
      shouldAutoScrollToBottom({
        followMode: "follow",
        isAtBottom: true,
        forceStickToBottom: false,
      }),
    ).toBe(true);
  });

  it("blocks auto-scroll when the user has left the bottom even if follow mode is stale", () => {
    expect(
      shouldAutoScrollToBottom({
        followMode: "follow",
        isAtBottom: false,
        forceStickToBottom: false,
      }),
    ).toBe(false);
  });

  it("blocks auto-scroll while detached", () => {
    expect(
      shouldAutoScrollToBottom({
        followMode: "detached",
        isAtBottom: true,
        forceStickToBottom: false,
      }),
    ).toBe(false);
  });

  it("still allows explicit force-stick requests", () => {
    expect(
      shouldAutoScrollToBottom({
        followMode: "detached",
        isAtBottom: false,
        forceStickToBottom: true,
      }),
    ).toBe(true);
  });
});

describe("shouldRunScheduledBottomScroll", () => {
  it("drops a queued auto-scroll when the user leaves the bottom before it runs", () => {
    expect(
      shouldRunScheduledBottomScroll({
        followMode: "detached",
        isAtBottom: false,
        forceStickToBottom: false,
        explicitForce: false,
      }),
    ).toBe(false);
  });

  it("keeps explicit scroll-to-bottom actions working while detached", () => {
    expect(
      shouldRunScheduledBottomScroll({
        followMode: "detached",
        isAtBottom: false,
        forceStickToBottom: false,
        explicitForce: true,
      }),
    ).toBe(true);
  });
});
