import { describe, expect, it } from "vitest";

import type { LedgerEvent } from "../types";
import {
  shouldAutoScrollToBottom,
  shouldPromoteScrollToFollow,
} from "./virtualMessageListHelpers";
import { buildGroupBridgeDisplayNameMap } from "./virtualMessageListGroupBridge";

describe("buildGroupBridgeDisplayNameMap", () => {
  it("maps group_bridge chat senders to the remote group name from message metadata", () => {
    const messages: LedgerEvent[] = [
      {
        kind: "chat.message",
        by: "group_bridge:peer_remote",
        data: { source_user_name: "Remote Group" },
      },
    ];

    expect(buildGroupBridgeDisplayNameMap(messages).get("group_bridge:peer_remote")).toBe("Remote Group");
  });

  it("ignores non-chat events and group_bridge messages without a source name", () => {
    const messages: LedgerEvent[] = [
      {
        kind: "chat.read",
        by: "group_bridge:peer_read",
        data: { source_user_name: "Read Group" },
      },
      {
        kind: "chat.message",
        by: "group_bridge:peer_without_name",
        data: { text: "hello" },
      },
    ];

    expect(buildGroupBridgeDisplayNameMap(messages).size).toBe(0);
  });
});

describe("virtual message list follow promotion", () => {
  it("does not promote detached history reading back to follow on a non-user at-bottom scroll event", () => {
    expect(
      shouldPromoteScrollToFollow({
        followMode: "detached",
        previousTop: 420,
        currentTop: 420,
      })
    ).toBe(false);
  });

  it("promotes detached history reading back to follow only when the user scrolls down to bottom", () => {
    expect(
      shouldPromoteScrollToFollow({
        followMode: "detached",
        previousTop: 420,
        currentTop: 480,
      })
    ).toBe(true);
  });

  it("keeps an existing follow session in follow across at-bottom scroll events", () => {
    expect(
      shouldPromoteScrollToFollow({
        followMode: "follow",
        previousTop: 480,
        currentTop: 480,
      })
    ).toBe(true);
  });
});

describe("virtual message list tail append auto-scroll", () => {
  it("does not auto-scroll a detached history reader when an AI message is appended", () => {
    expect(
      shouldAutoScrollToBottom({
        followMode: "detached",
        isAtBottom: true,
        forceStickToBottom: false,
      })
    ).toBe(false);
  });

  it("keeps bottom follow users pinned when an AI message is appended", () => {
    expect(
      shouldAutoScrollToBottom({
        followMode: "follow",
        isAtBottom: true,
        forceStickToBottom: false,
      })
    ).toBe(true);
  });
});
