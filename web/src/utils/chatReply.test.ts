import { describe, expect, it } from "vite-plus/test";

import { buildReplyComposerState } from "./chatReply";
import type { LedgerEvent } from "../types";

describe("buildReplyComposerState", () => {
  it("replies to canonical recipients from a Rust cross-group source event", () => {
    const event: LedgerEvent = {
      id: "evt_source",
      kind: "chat.message",
      by: "user",
      data: { text: "hello remote", to: ["@foreman"], dst_group_id: "g_remote" },
    };

    const state = buildReplyComposerState(event, "g_local", [], {
      default_send_to: "broadcast",
    } as never);

    expect(state?.destGroupId).toBe("g_remote");
    expect(state?.toText).toBe("@foreman");
  });

  it("disables replies to retired Group Bridge messages", () => {
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

    const state = buildReplyComposerState(event, "g_local", [], {
      default_send_to: "foreman",
    } as never);

    expect(state).toBeNull();
  });

  it("does not route historical remote replies to a same-ID local Actor", () => {
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

    const state = buildReplyComposerState(event, "g_local", [{ id: "peer1" } as never], {
      default_send_to: "foreman",
    } as never);

    expect(state).toBeNull();
  });
});

it.each([
  { src_instance_id: "other-instance", src_group_id: "g_local", src_event_id: "remote-event" },
  {
    dst_instance_id: "other-instance",
    dst_group_id: "g_local",
    dst_to: ["worker"],
    remote_event_id: "remote-event",
  },
])("keeps Connect replies anchored to the local event despite colliding Group IDs", (route) => {
  const state = buildReplyComposerState(
    {
      id: "local-event",
      kind: "chat.message",
      by: "user",
      data: { ...route, text: "question", to: ["worker"] },
    },
    "g_local",
    [{ id: "worker", title: "Unrelated local worker" } as never],
    undefined,
  );
  expect(state).toMatchObject({
    destGroupId: "g_local",
    toText: "",
    replyTarget: { eventId: "local-event" },
  });
  expect(state?.replyTarget.remoteDstGroupId).toBeUndefined();
  expect(state?.replyTarget.remoteReplyToEventId).toBeUndefined();
});

it("leaves a reply to Chet without recipients, naming Chet only for this Group's current marker", () => {
  const event = (marker: string) => ({
    id: "chet-event",
    kind: "chat.message",
    by: "svc:chet",
    data: { text: "Please check the build.", to: ["user"], _cccc_chet_group_generation: marker },
  });
  const reply = (marker: string) =>
    buildReplyComposerState(
      event(marker),
      "g_local",
      [],
      { default_send_to: "foreman" } as never,
      "gen-2",
    );
  expect(reply("gen-2")).toMatchObject({
    toText: "",
    replyTarget: { eventId: "chet-event", by: "Chet", toChet: true },
  });
  // A copied or stale message keeps its bare principal, and still falls back to no one.
  expect(reply("gen-1")).toMatchObject({
    toText: "",
    replyTarget: { eventId: "chet-event", by: "svc:chet", toChet: true },
  });
  // A reply to anyone else can still go as Mail.
  const other = buildReplyComposerState(
    { id: "m1", kind: "chat.message", by: "peer", data: { text: "hi", to: ["user"] } },
    "g_local",
    [{ id: "peer" }] as never,
    { default_send_to: "foreman" } as never,
    "gen-2",
  );
  expect(other?.replyTarget.toChet).toBeUndefined();
});
