import { describe, expect, it } from "vitest";

import type { LedgerEvent } from "../types";
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
