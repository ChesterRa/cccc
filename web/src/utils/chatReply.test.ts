import { describe, expect, it } from "vitest";

import { buildReplyComposerState } from "./chatReply";
import type { LedgerEvent } from "../types";

describe("buildReplyComposerState", () => {
  it("does not prefill local recipients when replying to group_bridge messages", () => {
    const event: LedgerEvent = {
      id: "evt_local",
      kind: "chat.message",
      by: "group_bridge:peer_remote",
      data: {
        text: "hello from remote",
        to: ["@foreman"],
        source_platform: "group_bridge_session",
        source_user_id: "peer_remote",
        src_group_id: "g_remote",
        src_event_id: "evt_remote",
      },
    };

    const state = buildReplyComposerState(event, "g_local", [], { default_send_to: "foreman" } as never);

    expect(state?.toText).toBe("");
    expect(state?.replyTarget?.eventId).toBe("evt_local");
  });

  it("treats local replies with inherited group_bridge metadata as local actor messages", () => {
    const event: LedgerEvent = {
      id: "evt_reply",
      kind: "chat.message",
      by: "peer1",
      data: {
        text: "local actor reply",
        to: ["user"],
        reply_to: "evt_remote_copy",
        source_platform: "group_bridge_session",
        source_user_name: "Remote group",
        source_user_id: "peer_remote",
      },
    };

    const state = buildReplyComposerState(
      event,
      "g_local",
      [{ id: "peer1" } as never],
      { default_send_to: "foreman" } as never,
    );

    expect(state?.toText).toBe("peer1");
    expect(state?.replyTarget?.eventId).toBe("evt_reply");
  });
});
