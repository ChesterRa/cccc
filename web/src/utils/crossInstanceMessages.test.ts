import { describe, expect, it } from "vite-plus/test";

import { isCrossInstanceInboundMessage } from "./crossInstanceMessages";

describe("isCrossInstanceInboundMessage", () => {
  it("accepts messages authored by the Group Bridge transport", () => {
    expect(isCrossInstanceInboundMessage("group_bridge:peer_remote", {})).toBe(true);
  });

  it("accepts legacy Group Bridge messages with source metadata and a source group", () => {
    expect(
      isCrossInstanceInboundMessage("unknown", {
        source_platform: "group_bridge_session",
        src_group_id: "g_remote",
      }),
    ).toBe(true);
  });

  it("does not classify local replies that only inherited source metadata as remote", () => {
    expect(
      isCrossInstanceInboundMessage("peer1", {
        source_platform: "group_bridge_session",
        source_user_name: "Remote group",
        source_user_id: "peer_remote",
      }),
    ).toBe(false);
  });
});

it("identifies Connect transport authors without classifying local replies as inbound", () => {
  expect(
    isCrossInstanceInboundMessage("connect:instance-b", { src_instance_id: "instance-b" }),
  ).toBe(true);
  expect(isCrossInstanceInboundMessage("worker", { source_platform: "cccc_connect" })).toBe(false);
});
