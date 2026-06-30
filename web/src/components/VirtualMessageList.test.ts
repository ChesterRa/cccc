import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import type { LedgerEvent } from "../types";
import { shouldKeepFollowDuringContentResize } from "./virtualMessageListHelpers";
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

describe("virtual message list resize follow behavior", () => {
  it("routes content resize anchoring through an explicit follow-preservation guard", () => {
    const listSource = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "VirtualMessageList.tsx"), "utf8");

    expect(listSource).toContain("shouldKeepFollowDuringContentResize");
    expect(listSource).toContain("isContainerResizingRef.current = true");
    expect(listSource).not.toContain("shouldAdjustScrollPositionOnItemSizeChange");
  });

  it("keeps follow mode through content height changes while the previous viewport was at bottom", () => {
    expect(
      shouldKeepFollowDuringContentResize({
        followMode: "follow",
        wasAtBottomBeforeResize: true,
        contentSizeChanged: true,
      })
    ).toBe(true);
  });

  it("does not force follow during resize when the user is already detached", () => {
    expect(
      shouldKeepFollowDuringContentResize({
        followMode: "detached",
        wasAtBottomBeforeResize: true,
        contentSizeChanged: true,
      })
    ).toBe(false);
  });

  it("does not force follow during resize when content size did not change", () => {
    expect(
      shouldKeepFollowDuringContentResize({
        followMode: "follow",
        wasAtBottomBeforeResize: true,
        contentSizeChanged: false,
      })
    ).toBe(false);
  });
});
