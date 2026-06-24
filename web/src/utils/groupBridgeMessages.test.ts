import { describe, expect, it } from "vitest";

import { isGroupBridgeInboundMessage } from "./groupBridgeMessages";

describe("isGroupBridgeInboundMessage", () => {
  it("accepts messages authored by the Group Bridge transport", () => {
    expect(isGroupBridgeInboundMessage("group_bridge:peer_remote", {})).toBe(true);
  });

  it("accepts legacy Group Bridge messages with source metadata and a source group", () => {
    expect(isGroupBridgeInboundMessage("unknown", {
      source_platform: "group_bridge_session",
      src_group_id: "g_remote",
    })).toBe(true);
  });

  it("does not classify local replies that only inherited source metadata as remote", () => {
    expect(isGroupBridgeInboundMessage("peer1", {
      source_platform: "group_bridge_session",
      source_user_name: "Remote group",
      source_user_id: "peer_remote",
    })).toBe(false);
  });
});
