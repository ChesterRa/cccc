import { describe, expect, it, vi } from "vitest";

const { localStorageMock } = vi.hoisted(() => {
  function makeStorage() {
    const data = new Map<string, string>();
    return {
      getItem: vi.fn((key: string) => data.get(key) ?? null),
      setItem: vi.fn((key: string, value: string) => {
        data.set(key, String(value));
      }),
      removeItem: vi.fn((key: string) => {
        data.delete(key);
      }),
      clear: vi.fn(() => {
        data.clear();
      }),
    };
  }

  const storage = makeStorage();
  vi.stubGlobal("localStorage", storage);
  vi.stubGlobal("window", { setTimeout, clearTimeout, localStorage: storage });
  return { localStorageMock: storage };
});

import {
  CHAT_SCROLL_SNAPSHOT_MAX_AGE_MS,
  buildComposerTrustFetchGroupId,
  buildUnfilteredLiveChatMessages,
  buildComposerSendRoutingSnapshot,
  buildComposerSendRecipientTokens,
  buildReplyAnchorTsMap,
  buildReplySlotTsMap,
  collapseActorStreamingPlaceholders,
  dedupeStreamingEvents,
  mergeVisibleChatMessages,
  parseComposerRecipientTokens,
  restoreFailedSendComposerState,
  shouldShowInConversation,
  sortChatMessages,
  shouldLockChatToBottomForSend,
  shouldRestoreDetachedScrollSnapshot,
  toVisibleConversationEvent,
} from "../../src/hooks/useChatTab";
import {
  useComposerStore,
} from "../../src/stores/useComposerStore";
import {
  useGroupStore,
} from "../../src/stores/useGroupStore";
import {
  formatSendMessageError,
  getGroupSendBlockedMessage,
  getGroupSendBlockedReason,
  isFormalChatMessageEvent,
  supportsChatStreamingPlaceholder,
} from "../../src/utils/chatSend";
import { latestSuggestedUserMessage } from "../../src/utils/suggestedUserMessage";
import type { LedgerEvent } from "../../src/types";

void localStorageMock;

const t = (key: string, options?: Record<string, unknown>) => String(options?.defaultValue || key);

const resetComposerAndGroupSelection = () => {
  useGroupStore.setState({ selectedGroupId: "" });
  useComposerStore.setState({
    activeGroupId: "",
    composerText: "",
    composerFiles: [],
    toText: "",
    replyTarget: null,
    quotedPresentationRef: null,
    priority: "normal",
    replyRequired: false,
    destGroupId: "",
    drafts: {},
    normalToTextByGroup: {},
  });
};

function makeStreamingEvent({
  id,
  by = "claude-1",
  streamId,
  pendingEventId,
  text = "",
  pendingPlaceholder = false,
}: {
  id: string;
  by?: string;
  streamId: string;
  pendingEventId?: string;
  text?: string;
  pendingPlaceholder?: boolean;
}): LedgerEvent {
  return {
    id,
    kind: "chat.message",
    by,
    _streaming: true,
    data: {
      text,
      to: ["user"],
      stream_id: streamId,
      pending_event_id: pendingEventId,
      pending_placeholder: pendingPlaceholder,
      activities: pendingPlaceholder
        ? [{ id: `queued:${id}`, kind: "queued", status: "started", summary: "queued" }]
        : [],
    },
  };
}

describe("dedupeStreamingEvents", () => {
  it("shows hidden slash skill dispatch events as the original slash command", () => {
    const event = {
      id: "slashskill-1",
      kind: "chat.message",
      by: "user",
      data: {
        text: "[CCCC] INTERNAL CONTROL",
        to: ["architect"],
        refs: [
          {
            kind: "text",
            title: "slash_skill_dispatch",
            hidden: true,
            control_kind: "slash_skill_dispatch",
            command: "/using-superpowers",
            capability_id: "skill:agent_self_proposed:using-superpowers",
            task_text: "开始执行",
          },
        ],
      },
    } as LedgerEvent;

    expect(shouldShowInConversation(event)).toBe(true);
    const visible = toVisibleConversationEvent(event);
    expect((visible.data as { text?: string }).text).toBe("/using-superpowers 开始执行");
    expect((visible.data as { refs?: unknown[] }).refs).toEqual([]);
  });

  it("keeps distinct text-bearing stream ids for the same pending event", () => {
    const events = dedupeStreamingEvents([
      makeStreamingEvent({
        id: "stream:commentary-1",
        streamId: "commentary-1",
        pendingEventId: "evt-1",
        text: "先查一下",
      }),
      makeStreamingEvent({
        id: "stream:commentary-2",
        streamId: "commentary-2",
        pendingEventId: "evt-1",
        text: "继续看",
      }),
    ]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => String(event.id || ""))).toEqual([
      "stream:commentary-1",
      "stream:commentary-2",
    ]);
  });

  it("collapses duplicate pending placeholders for the same pending event", () => {
    const events = dedupeStreamingEvents([
      makeStreamingEvent({
        id: "pending:evt-1:claude-1:a",
        streamId: "pending:evt-1:claude-1:a",
        pendingEventId: "evt-1",
        pendingPlaceholder: true,
      }),
      makeStreamingEvent({
        id: "pending:evt-1:claude-1:b",
        streamId: "pending:evt-1:claude-1:b",
        pendingEventId: "evt-1",
        pendingPlaceholder: true,
      }),
    ]);

    expect(events).toHaveLength(1);
    expect(String((events[0]?.data as { pending_event_id?: string }).pending_event_id || "")).toBe("evt-1");
  });

  it("keeps a real stream separate from its queued placeholder before later collapse", () => {
    const events = dedupeStreamingEvents([
      makeStreamingEvent({
        id: "pending:evt-1:claude-1",
        streamId: "pending:evt-1:claude-1",
        pendingEventId: "evt-1",
        pendingPlaceholder: true,
      }),
      makeStreamingEvent({
        id: "stream:final-1",
        streamId: "final-1",
        pendingEventId: "evt-1",
        text: "最终回复",
      }),
    ]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => String(event.id || ""))).toEqual([
      "pending:evt-1:claude-1",
      "stream:final-1",
    ]);
  });

  it("keeps explicit commentary streams separate even if their ids still look pending-like", () => {
    const events = dedupeStreamingEvents([
      makeStreamingEvent({
        id: "pending:evt-2:claude-1",
        streamId: "pending:evt-2:claude-1",
        pendingEventId: "evt-2",
        pendingPlaceholder: true,
      }),
      {
        id: "stream:commentary-2",
        kind: "chat.message",
        by: "claude-1",
        _streaming: true,
        data: {
          text: "先同步一下排查路径",
          to: ["user"],
          stream_id: "pending:evt-2:claude-1:commentary",
          pending_event_id: "evt-2",
          pending_placeholder: false,
          stream_phase: "commentary",
          activities: [],
        },
      },
    ]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => String(event.id || ""))).toEqual([
      "pending:evt-2:claude-1",
      "stream:commentary-2",
    ]);
  });
});

describe("buildComposerSendRoutingSnapshot", () => {
  it("uses the selected group while the composer still belongs to the previous group", () => {
    const snapshot = buildComposerSendRoutingSnapshot({
      selectedGroupId: "g-new",
      activeGroupId: "g-old",
      destGroupId: "g-old-remote",
    });

    expect(snapshot).toEqual({
      selectedGroupId: "g-new",
      destGroupId: "g-new",
      composerGroupSettled: false,
      isCrossGroup: false,
    });
  });

  it("marks explicit cross-group delivery only after composer ownership is settled", () => {
    const snapshot = buildComposerSendRoutingSnapshot({
      selectedGroupId: "g-current",
      activeGroupId: "g-current",
      destGroupId: "g-dst",
    });

    expect(snapshot).toMatchObject({
      selectedGroupId: "g-current",
      destGroupId: "g-dst",
      composerGroupSettled: true,
      isCrossGroup: true,
    });
  });
});

describe("buildComposerTrustFetchGroupId", () => {
  it("fetches only the selected group's trusts for # routing suggestions", () => {
    expect(buildComposerTrustFetchGroupId("g_current")).toBe("g_current");
    expect(buildComposerTrustFetchGroupId("")).toBeUndefined();
  });
});

describe("buildComposerSendRecipientTokens", () => {
  it("keeps current-group actor tokens for same-group sends", () => {
    expect(buildComposerSendRecipientTokens({
      toText: "peer-architect, @foreman, missing",
      isCrossGroup: false,
      validRecipientSet: new Set(["peer-architect", "@foreman"]),
      crossGroupValidRecipientSet: new Set(["@foreman"]),
    })).toEqual(["peer-architect", "@foreman"]);
  });

  it("filters current-group actor tokens from cross-group sends", () => {
    expect(buildComposerSendRecipientTokens({
      toText: "peer-architect, @foreman, target-peer",
      isCrossGroup: true,
      validRecipientSet: new Set(["peer-architect", "@foreman", "target-peer"]),
      crossGroupValidRecipientSet: new Set(["@foreman", "target-peer"]),
    })).toEqual(["@foreman", "target-peer"]);
  });
});

describe("buildUnfilteredLiveChatMessages", () => {
  it("keeps later user replies in the suggestion freshness source even when a view could be filtered", () => {
    const events: LedgerEvent[] = [
      {
        id: "evt-agent",
        kind: "chat.message",
        ts: "2026-06-15T00:00:01Z",
        by: "agent-1",
        data: {
          text: "Done.",
          to: ["user"],
          suggested_user_message: "Please continue.",
        },
      },
      {
        id: "evt-user",
        kind: "chat.message",
        ts: "2026-06-15T00:00:02Z",
        by: "user",
        data: {
          text: "I already continued.",
          to: ["agent-1"],
        },
      },
    ];

    const source = buildUnfilteredLiveChatMessages(events, [], { map: new Map(), next: 0 });

    expect(source.map((event) => event.id)).toEqual(["evt-agent", "evt-user"]);
    expect(latestSuggestedUserMessage(source)).toBeNull();
  });
});

describe("restoreFailedSendComposerState", () => {
  it("restores the current composer when the failed send still belongs to the selected group", () => {
    resetComposerAndGroupSelection();
    useGroupStore.setState({ selectedGroupId: "g-a" });
    useComposerStore.getState().switchGroup(null, "g-a");
    useComposerStore.getState().setComposerText("");

    restoreFailedSendComposerState({
      originGroupId: "g-a",
      composerText: "failed message",
      composerFiles: [],
      toText: "@foreman",
      replyTarget: { eventId: "evt-1", by: "peer-1", text: "prior" },
      quotedPresentationRef: null,
      priority: "attention",
      replyRequired: true,
    });

    const state = useComposerStore.getState();
    expect(state.composerText).toBe("failed message");
    expect(state.activeGroupId).toBe("g-a");
    expect(state.toText).toBe("@foreman");
    expect(state.replyTarget).toMatchObject({ eventId: "evt-1" });
    expect(state.priority).toBe("attention");
    expect(state.replyRequired).toBe(true);
  });

  it("stores the failed send as the origin group draft when selection moved away", () => {
    resetComposerAndGroupSelection();
    const file = new File(["hello"], "hello.txt", { type: "text/plain" });
    useGroupStore.setState({ selectedGroupId: "g-b" });
    useComposerStore.getState().switchGroup(null, "g-b");
    useComposerStore.getState().setComposerText("current b text");

    restoreFailedSendComposerState({
      originGroupId: "g-a",
      composerText: "failed a message",
      composerFiles: [file],
      toText: "@foreman",
      replyTarget: { eventId: "evt-a", by: "peer-a", text: "prior a" },
      quotedPresentationRef: {
        kind: "presentation_ref",
        slot_id: "slot-1",
        title: "Deck",
      },
      priority: "attention",
      replyRequired: true,
    });

    const state = useComposerStore.getState();
    expect(state.activeGroupId).toBe("g-b");
    expect(state.composerText).toBe("current b text");
    expect(state.drafts["g-a"]).toMatchObject({
      composerText: "failed a message",
      composerFiles: [file],
      toText: "@foreman",
      replyTarget: { eventId: "evt-a", by: "peer-a", text: "prior a" },
      quotedPresentationRef: {
        kind: "presentation_ref",
        slot_id: "slot-1",
        title: "Deck",
      },
      priority: "attention",
      replyRequired: true,
    });
  });
});

describe("parseComposerRecipientTokens", () => {
  it("deduplicates recipient tokens from the same composer snapshot", () => {
    expect(parseComposerRecipientTokens("@foreman, @, peer-1, peer-1, missing", new Set(["@foreman", "peer-1"]))).toEqual([
      "@foreman",
      "peer-1",
    ]);
  });
});

describe("isFormalChatMessageEvent", () => {
  it("keeps headless streaming events out of the standard chat message list", () => {
    expect(isFormalChatMessageEvent({
      id: "stream:commentary-1",
      kind: "chat.message",
      by: "coder",
      _streaming: true,
      data: {
        text: "正在排查",
        stream_id: "commentary-1",
        to: ["user"],
      },
    })).toBe(false);

    expect(isFormalChatMessageEvent({
      id: "evt-formal-1",
      kind: "chat.message",
      by: "coder",
      data: {
        text: "正式回复",
        to: ["user"],
      },
    })).toBe(true);
  });
});

describe("group send blocked state", () => {
  it("blocks only explicit paused lifecycle states before optimistic send feedback", () => {
    expect(getGroupSendBlockedReason({
      lifecycleState: "paused",
      runtimeRunning: true,
      actorCount: 2,
    })).toBe("paused");
    expect(getGroupSendBlockedReason({
      lifecycleState: "active",
      runtimeRunning: false,
      actorCount: 2,
    })).toBeNull();
    expect(getGroupSendBlockedReason({
      lifecycleState: "stopped",
      runtimeRunning: false,
      actorCount: 2,
    })).toBeNull();
    expect(getGroupSendBlockedReason({
      lifecycleState: "idle",
      runtimeRunning: true,
      actorCount: 2,
    })).toBeNull();
  });

  it("maps recipient-resolution failures to group-state recovery copy", () => {
    expect(getGroupSendBlockedMessage("paused", t)).toBe(
      "This group is paused. Resume the group before sending a message to agents.",
    );
    expect(formatSendMessageError({
      code: "no_enabled_recipients",
      message: "No enabled recipients after excluding sender.",
      groupSendBlockedReason: "paused",
      t,
    })).toBe("This group is paused. Resume the group before sending a message to agents.");
    expect(formatSendMessageError({
      code: "invalid_recipient",
      message: "unknown recipient",
      groupSendBlockedReason: "paused",
      t,
    })).toBe("invalid_recipient: unknown recipient");
  });
});

describe("collapseActorStreamingPlaceholders", () => {
  it("keeps process-only pending bubbles when the same actor already has real streaming text", () => {
    const events = collapseActorStreamingPlaceholders([
      makeStreamingEvent({
        id: "stream:commentary-1",
        streamId: "commentary-1",
        pendingEventId: "evt-1",
        text: "我在查前端合并链路",
      }),
      {
        id: "pending:evt-1:claude-1",
        kind: "chat.message",
        by: "claude-1",
        _streaming: true,
        data: {
          text: "",
          to: ["user"],
          stream_id: "pending:evt-1:claude-1",
          pending_event_id: "evt-1",
          pending_placeholder: true,
          activities: [{ id: "tool-1", kind: "tool", status: "started", summary: "chrome-devtools:take_snapshot" }],
        },
      },
    ]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => String(event.id || ""))).toEqual([
      "stream:commentary-1",
      "pending:evt-1:claude-1",
    ]);
  });

  it("keeps a new local queued placeholder when the actor only has rich streaming from an older reply slot", () => {
    const events = collapseActorStreamingPlaceholders([
      makeStreamingEvent({
        id: "stream:old-final",
        streamId: "old-final",
        pendingEventId: "evt-0",
        text: "上一轮已经有正文",
      }),
      {
        id: "stream:queued-local",
        kind: "chat.message",
        by: "claude-1",
        _streaming: true,
        data: {
          text: "",
          to: ["user"],
          stream_id: "local:msg-2:claude-1",
          pending_placeholder: true,
          activities: [{ id: "queued:2", kind: "queued", status: "started", summary: "queued" }],
        },
      },
    ]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => String(event.id || ""))).toContain("stream:queued-local");
  });

  it("drops empty same-slot stream variants once real streaming text exists", () => {
    const events = collapseActorStreamingPlaceholders([
      makeStreamingEvent({
        id: "stream:commentary-1",
        streamId: "commentary-1",
        pendingEventId: "evt-1",
        text: "我先继续查前端合并链路",
      }),
      makeStreamingEvent({
        id: "stream:final-empty-1",
        streamId: "final-empty-1",
        pendingEventId: "evt-1",
        text: "",
      }),
    ]);

    expect(events).toHaveLength(1);
    expect(String(events[0]?.id || "")).toBe("stream:commentary-1");
  });

  it("keeps explicit phase streams even when their first frame is queued-only", () => {
    const events = collapseActorStreamingPlaceholders([
      {
        id: "stream:commentary-queued",
        kind: "chat.message",
        by: "claude-1",
        _streaming: true,
        data: {
          text: "",
          to: ["user"],
          stream_id: "pending:evt-2:claude-1:commentary",
          pending_event_id: "evt-2",
          pending_placeholder: false,
          stream_phase: "commentary",
          activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
        },
      },
      makeStreamingEvent({
        id: "stream:final-2",
        streamId: "final-2",
        pendingEventId: "evt-2",
        text: "给出最终结论",
      }),
    ]);

    expect(events).toHaveLength(2);
    expect(events.map((event) => String(event.id || ""))).toContain("stream:commentary-queued");
    expect(events.map((event) => String(event.id || ""))).toContain("stream:final-2");
  });
});

describe("sortChatMessages", () => {
  it("keeps finalized replies in their original streaming slot order", () => {
    const streaming: LedgerEvent[] = [
      {
        id: "stream:a",
        ts: "2026-04-04T15:29:00.100Z",
        kind: "chat.message",
        by: "backend-expert",
        _streaming: true,
        data: {
          text: "",
          pending_event_id: "user-msg-1",
          stream_id: "stream-a",
          to: ["user"],
        },
      },
      {
        id: "stream:b",
        ts: "2026-04-04T15:29:00.200Z",
        kind: "chat.message",
        by: "project-director",
        _streaming: true,
        data: {
          text: "",
          pending_event_id: "user-msg-2",
          stream_id: "stream-b",
          to: ["user"],
        },
      },
    ];
    const canonical: LedgerEvent[] = [
      {
        id: "evt-b",
        ts: "2026-04-04T15:29:03.000Z",
        kind: "chat.message",
        by: "project-director",
        data: {
          text: "第二条先完成",
          reply_to: "user-msg-2",
          stream_id: "stream-b",
          to: ["user"],
        },
      },
      {
        id: "evt-a",
        ts: "2026-04-04T15:29:04.000Z",
        kind: "chat.message",
        by: "backend-expert",
        data: {
          text: "第一条后完成",
          reply_to: "user-msg-1",
          stream_id: "stream-a",
          to: ["user"],
        },
      },
    ];

    const ordered = sortChatMessages(canonical, buildReplySlotTsMap(streaming));

    expect(ordered.map((event) => String(event.id || ""))).toEqual(["evt-a", "evt-b"]);
  });

  it("anchors assistant placeholders after the user message they are replying to", () => {
    const pendingUserMessage: LedgerEvent = {
      id: "local-user-1",
      ts: "2026-04-08T06:04:10.000Z",
      kind: "chat.message",
      by: "user",
      data: {
        text: "为什么当前点击启动，但是前端ui没变？",
        to: ["@foreman"],
        client_id: "local-user-1",
        _optimistic: true,
      },
    };
    const assistantPlaceholder: LedgerEvent = {
      id: "stream:queued-1",
      ts: "2026-04-08T06:04:09.000Z",
      kind: "chat.message",
      by: "requirements-expert",
      _streaming: true,
      data: {
        text: "",
        to: ["user"],
        stream_id: "local:local-user-1:requirements-expert",
        pending_event_id: "local-user-1",
        pending_placeholder: true,
        activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
      },
    };

    const ordered = sortChatMessages(
      [assistantPlaceholder, pendingUserMessage],
      buildReplyAnchorTsMap([pendingUserMessage], [assistantPlaceholder]),
    );

    expect(ordered.map((event) => String(event.id || ""))).toEqual(["local-user-1", "stream:queued-1"]);
  });
});

describe("mergeLiveChatMessageEvents", () => {
  it("keeps the optimistic user bubble visible until the canonical echo is renderable", () => {
    const optimisticEvent: LedgerEvent = {
      id: "local-1",
      ts: "2026-04-05T02:00:00.000Z",
      kind: "chat.message",
      by: "user",
      data: {
        text: "马上发出去",
        to: ["@foreman"],
        client_id: "local-1",
        _optimistic: true,
      },
    };
    const nonRenderableCanonical: LedgerEvent = {
      id: "evt-1",
      ts: "2026-04-05T02:00:01.000Z",
      kind: "chat.message",
      by: "user",
      data: {
        text: "",
        to: ["@foreman"],
        client_id: "local-1",
      },
    };

    const merged = mergeVisibleChatMessages(
      [nonRenderableCanonical],
      [],
      [optimisticEvent],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe("local-1");
    expect(String(((merged[0]?.data as { text?: string })?.text) || "")).toBe("马上发出去");
  });

  it("does not collapse multiple canonical replies from the same actor to the same parent", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-1",
          ts: "2026-04-04T16:28:01.000Z",
          kind: "chat.message",
          by: "requirements-expert",
          data: {
            text: "我先查",
            reply_to: "user-msg-1",
            to: ["user"],
          },
        },
        {
          id: "evt-2",
          ts: "2026-04-04T16:28:05.000Z",
          kind: "chat.message",
          by: "requirements-expert",
          data: {
            text: "查到了，继续给结论",
            reply_to: "user-msg-1",
            to: ["user"],
          },
        },
      ],
      [],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged.map((event) => String(event.id || ""))).toEqual(["evt-1", "evt-2"]);
  });

  it("keeps commentary streaming text when canonical message with the same stream id is still empty", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-empty",
          ts: "2026-04-04T15:29:03.000Z",
          kind: "chat.message",
          by: "claude-1",
          data: {
            text: "",
            reply_to: "user-msg-1",
            stream_id: "stream-commentary",
            to: ["user"],
          },
        },
      ],
      [
        {
          id: "stream:commentary",
          ts: "2026-04-04T15:29:00.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "我先检查 commentary 合并",
            pending_event_id: "user-msg-1",
            stream_id: "stream-commentary",
            to: ["user"],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(merged.map((event) => String(event.id || ""))).toEqual(["stream:commentary"]);
    expect(String((merged[0]?.data as { text?: unknown })?.text || "")).toBe("我先检查 commentary 合并");
  });

  it("drops streaming commentary once canonical message with the same stream id has renderable content", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-final",
          ts: "2026-04-04T15:29:03.000Z",
          kind: "chat.message",
          by: "claude-1",
          data: {
            text: "已经定位到问题",
            reply_to: "user-msg-1",
            stream_id: "stream-commentary",
            to: ["user"],
          },
        },
      ],
      [
        {
          id: "stream:commentary",
          ts: "2026-04-04T15:29:00.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "我先检查 commentary 合并",
            pending_event_id: "user-msg-1",
            stream_id: "stream-commentary",
            to: ["user"],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe("evt-final");
    expect(String((merged[0]?.data as { text?: unknown })?.text || "")).toBe("已经定位到问题");
  });

  it("keeps a fresh local queued placeholder visible even when the actor already has older canonical replies", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-old",
          ts: "2026-04-04T15:29:03.000Z",
          kind: "chat.message",
          by: "claude-1",
          data: {
            text: "上一轮已经回复过",
            reply_to: "user-msg-0",
            to: ["user"],
          },
        },
      ],
      [
        {
          id: "stream:queued-local",
          ts: "2026-04-04T15:29:02.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "",
            stream_id: "local:msg-1:claude-1",
            pending_event_id: "local_1",
            pending_placeholder: true,
            to: ["user"],
            activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(2);
    expect(merged.map((event) => String(event.id || ""))).toContain("stream:queued-local");
  });

  it("keeps pending and real streaming variants in one logical slot", () => {
    const merged = mergeVisibleChatMessages(
      [],
      [
        {
          id: "stream:local",
          ts: "2026-04-04T15:29:00.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "",
            pending_event_id: "user-msg-1",
            stream_id: "pending:user-msg-1:claude-1",
            pending_placeholder: true,
            to: ["user"],
            activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
          },
        },
        {
          id: "stream:real",
          ts: "2026-04-04T15:29:01.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "开始输出",
            pending_event_id: "user-msg-1",
            stream_id: "stream-final-1",
            pending_placeholder: false,
            to: ["user"],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe("stream:real");
  });

  it("keeps the streaming placeholder instead of replacing it with an empty canonical reply", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-empty",
          ts: "2026-04-04T15:29:01.000Z",
          kind: "chat.message",
          by: "claude-1",
          data: {
            text: "",
            reply_to: "user-msg-1",
            to: ["user"],
          },
        },
      ],
      [
        {
          id: "stream:queued-local",
          ts: "2026-04-04T15:29:00.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "",
            pending_event_id: "user-msg-1",
            stream_id: "local:msg-1:claude-1",
            pending_placeholder: true,
            to: ["user"],
            activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe("stream:queued-local");
  });

  it("treats two local placeholders from the same actor as different logical reply slots", () => {
    const merged = mergeVisibleChatMessages(
      [],
      [
        {
          id: "stream:queued-local-1",
          ts: "2026-04-04T15:29:00.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "",
            pending_event_id: "local_1",
            stream_id: "local:msg-1:claude-1",
            pending_placeholder: true,
            to: ["user"],
            activities: [{ id: "queued:1", kind: "queued", status: "started", summary: "queued" }],
          },
        },
        {
          id: "stream:queued-local-2",
          ts: "2026-04-04T15:29:02.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "",
            pending_event_id: "local_2",
            stream_id: "local:msg-2:claude-1",
            pending_placeholder: true,
            to: ["user"],
            activities: [{ id: "queued:2", kind: "queued", status: "started", summary: "queued" }],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(2);
    expect(merged.map((event) => String(event.id || ""))).toEqual([
      "stream:queued-local-1",
      "stream:queued-local-2",
    ]);
  });

  it("keeps multiple text-bearing streaming messages from the same actor and pending event", () => {
    const merged = mergeVisibleChatMessages(
      [],
      [
        {
          id: "stream:commentary-1",
          ts: "2026-04-04T15:29:00.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "先查一下",
            pending_event_id: "user-msg-1",
            stream_id: "commentary-1",
            to: ["user"],
          },
        },
        {
          id: "stream:commentary-2",
          ts: "2026-04-04T15:29:01.000Z",
          kind: "chat.message",
          by: "claude-1",
          _streaming: true,
          data: {
            text: "继续看",
            pending_event_id: "user-msg-1",
            stream_id: "commentary-2",
            to: ["user"],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged.map((event) => String(event.id || ""))).toEqual([
      "stream:commentary-1",
      "stream:commentary-2",
    ]);
  });
});

describe("mergeVisibleChatMessages", () => {
  it("collapses streaming and final assistant replies for the same reply slot into one bubble", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-final-assistant",
          ts: "2026-04-04T16:43:02.000Z",
          kind: "chat.message",
          by: "project-director",
          data: {
            text: "这是最终 canonical reply",
            reply_to: "user-msg-1",
            stream_id: "stream-reply-1",
            to: ["user"],
          },
        },
      ],
      [
        {
          id: "stream:partial-assistant",
          ts: "2026-04-04T16:43:01.000Z",
          kind: "chat.message",
          by: "project-director",
          _streaming: true,
          data: {
            text: "还在输出",
            pending_event_id: "user-msg-1",
            stream_id: "stream-reply-1",
            to: ["user"],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe("evt-final-assistant");
    expect(String((merged[0]?.data as { text?: unknown })?.text || "")).toBe("这是最终 canonical reply");
  });

  it("does not collapse multiple stream-backed replies from the same actor to the same parent", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-stream-1",
          ts: "2026-04-04T16:28:01.000Z",
          kind: "chat.message",
          by: "requirements-expert",
          data: {
            text: "我先查",
            reply_to: "user-msg-1",
            stream_id: "stream-1",
            to: ["user"],
          },
        },
        {
          id: "evt-stream-2",
          ts: "2026-04-04T16:28:05.000Z",
          kind: "chat.message",
          by: "requirements-expert",
          data: {
            text: "查到了，继续给结论",
            reply_to: "user-msg-1",
            stream_id: "stream-2",
            to: ["user"],
          },
        },
      ],
      [],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged.map((event) => String(event.id || ""))).toEqual(["evt-stream-1", "evt-stream-2"]);
  });

  it("keeps an optimistic user reply visible while the matching canonical event is still empty", () => {
    const localId = "local-user-reply-1";
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-empty-user",
          ts: "2026-04-04T16:40:02.000Z",
          kind: "chat.message",
          by: "user",
          data: {
            text: "",
            client_id: localId,
            reply_to: "agent-msg-1",
            to: ["@assistant"],
          },
        },
      ],
      [],
      [
        {
          id: localId,
          ts: "2026-04-04T16:40:01.000Z",
          kind: "chat.message",
          by: "user",
          data: {
            text: "这是 optimistic reply",
            client_id: localId,
            reply_to: "agent-msg-1",
            to: ["@assistant"],
            _optimistic: true,
          },
        },
      ],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe(localId);
    expect(String((merged[0]?.data as { text?: unknown })?.text || "")).toBe("这是 optimistic reply");
  });

  it("replaces the optimistic user reply once the canonical event has renderable content", () => {
    const localId = "local-user-reply-2";
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-user-final",
          ts: "2026-04-04T16:41:02.000Z",
          kind: "chat.message",
          by: "user",
          data: {
            text: "这是最终 canonical reply",
            client_id: localId,
            reply_to: "agent-msg-1",
            to: ["@assistant"],
          },
        },
      ],
      [],
      [
        {
          id: localId,
          ts: "2026-04-04T16:41:01.000Z",
          kind: "chat.message",
          by: "user",
          data: {
            text: "这是 optimistic reply",
            client_id: localId,
            reply_to: "agent-msg-1",
            to: ["@assistant"],
            _optimistic: true,
          },
        },
      ],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(1);
    expect(String(merged[0]?.id || "")).toBe("evt-user-final");
    expect(String((merged[0]?.data as { text?: unknown })?.text || "")).toBe("这是最终 canonical reply");
  });

  it("can be re-sorted by reply slot ts so a resumed streaming reply stays in its original slot after refresh", () => {
    const streaming: LedgerEvent[] = [
      {
        id: "stream:old-slot",
        ts: "2026-04-04T16:41:00.000Z",
        kind: "chat.message",
        by: "project-director",
        _streaming: true,
        data: {
          text: "继续输出旧槽位",
          pending_event_id: "user-msg-1",
          stream_id: "stream-old-slot",
          to: ["user"],
        },
      },
    ];
    const canonical: LedgerEvent[] = [
      {
        id: "evt-later",
        ts: "2026-04-04T16:42:00.000Z",
        kind: "chat.message",
        by: "user",
        data: {
          text: "后来的用户消息",
          to: ["project-director"],
        },
      },
    ];

    const merged = mergeVisibleChatMessages(
      canonical,
      streaming,
      [],
      { map: new Map(), next: 0 },
    );
    const ordered = sortChatMessages(merged, buildReplySlotTsMap(streaming));

    expect(ordered.map((event) => String(event.id || ""))).toEqual(["stream:old-slot", "evt-later"]);
  });

  it("keeps a non-queued activity bubble visible after the canonical reply for the same slot arrives", () => {
    const merged = mergeVisibleChatMessages(
      [
        {
          id: "evt-final",
          ts: "2026-04-04T16:43:02.000Z",
          kind: "chat.message",
          by: "project-director",
          data: {
            text: "这是最终 canonical reply",
            reply_to: "user-msg-1",
            stream_id: "stream-reply-1",
            to: ["user"],
          },
        },
      ],
      [
        {
          id: "pending:user-msg-1:project-director",
          ts: "2026-04-04T16:43:01.500Z",
          kind: "chat.message",
          by: "project-director",
          _streaming: true,
          data: {
            text: "",
            pending_event_id: "user-msg-1",
            stream_id: "pending:user-msg-1:project-director",
            pending_placeholder: true,
            to: ["user"],
            activities: [{ id: "cmd-1", kind: "command", status: "started", summary: "rg -n activity" }],
          },
        },
      ],
      [],
      { map: new Map(), next: 0 },
    );

    expect(merged).toHaveLength(2);
    expect(merged.map((event) => String(event.id || ""))).toEqual([
      "pending:user-msg-1:project-director",
      "evt-final",
    ]);
  });
});

describe("shouldRestoreDetachedScrollSnapshot", () => {
  it("restores only fresh detached snapshots with anchors", () => {
    const now = 1_700_000_000_000;
    expect(shouldRestoreDetachedScrollSnapshot({
      mode: "detached",
      anchorId: "evt-1",
      updatedAt: now - 1000,
    }, now)).toBe(true);

    expect(shouldRestoreDetachedScrollSnapshot({
      mode: "follow",
      anchorId: "",
      updatedAt: now,
    }, now)).toBe(false);

    expect(shouldRestoreDetachedScrollSnapshot({
      mode: "detached",
      anchorId: "evt-1",
      updatedAt: now - CHAT_SCROLL_SNAPSHOT_MAX_AGE_MS - 1,
    }, now)).toBe(false);

    expect(shouldRestoreDetachedScrollSnapshot({
      mode: "detached",
      anchorId: "",
      updatedAt: now,
    }, now)).toBe(false);
  });
});

describe("shouldLockChatToBottomForSend", () => {
  it("locks to bottom only when the current chat is cleanly following", () => {
    const now = 1_700_000_000_000;

    expect(shouldLockChatToBottomForSend({
      currentAtBottom: true,
      showScrollButton: false,
      chatUnreadCount: 0,
      scrollSnapshot: null,
      now,
    })).toBe(true);

    expect(shouldLockChatToBottomForSend({
      currentAtBottom: false,
      showScrollButton: false,
      chatUnreadCount: 0,
      scrollSnapshot: null,
      now,
    })).toBe(false);
  });

  it("does not force bottom while the user is browsing detached history", () => {
    const now = 1_700_000_000_000;

    expect(shouldLockChatToBottomForSend({
      currentAtBottom: true,
      showScrollButton: true,
      chatUnreadCount: 0,
      scrollSnapshot: null,
      now,
    })).toBe(false);

    expect(shouldLockChatToBottomForSend({
      currentAtBottom: true,
      showScrollButton: false,
      chatUnreadCount: 2,
      scrollSnapshot: null,
      now,
    })).toBe(false);

    expect(shouldLockChatToBottomForSend({
      currentAtBottom: true,
      showScrollButton: false,
      chatUnreadCount: 0,
      scrollSnapshot: {
        mode: "detached",
        anchorId: "evt-older",
        updatedAt: now - 1000,
      },
      now,
    })).toBe(false);
  });
});

describe("supportsChatStreamingPlaceholder", () => {
  it("returns true for codex headless actors", () => {
    expect(supportsChatStreamingPlaceholder({
      runtime: "codex",
      runner: "pty",
      runner_effective: "headless",
    })).toBe(true);
  });

  it("returns true for codex pty actors", () => {
    expect(supportsChatStreamingPlaceholder({
      runtime: "codex",
      runner: "pty",
      runner_effective: "pty",
    })).toBe(true);
  });

  it("returns true for non-codex known runtimes", () => {
    expect(supportsChatStreamingPlaceholder({
      runtime: "claude",
      runner: "headless",
      runner_effective: "headless",
    })).toBe(true);
  });

  it("returns false for custom runtime", () => {
    expect(supportsChatStreamingPlaceholder({
      runtime: "custom",
      runner: "pty",
      runner_effective: "pty",
    })).toBe(false);
  });

  it("returns false for empty runtime", () => {
    expect(supportsChatStreamingPlaceholder({
      runtime: "",
      runner: "pty",
      runner_effective: "pty",
    })).toBe(false);
  });
});
