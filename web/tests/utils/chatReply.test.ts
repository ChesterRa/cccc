import { describe, expect, it } from "vitest";

import { buildReplyComposerState } from "../../src/utils/chatReply";

describe("buildReplyComposerState", () => {
  it("prefers existing quote_text over message text when building a reply target", () => {
    const state = buildReplyComposerState(
      {
        id: "evt-1",
        kind: "chat.message",
        by: "user",
        data: {
          text: "测试activity消息抖动",
          quote_text: "为什么activity 会出现再消失，当前抖动太严重了",
          to: ["reviewer"],
        },
      } as any,
      "g-demo",
      [],
      null,
    );

    expect(state?.replyTarget.text).toBe("为什么activity 会出现再消失，当前抖动太严重了");
  });

  it("targets the original recipients when replying to a user-authored message", () => {
    const state = buildReplyComposerState(
      {
        id: "evt-2",
        kind: "chat.message",
        by: "user",
        data: {
          text: "问一问对方本周做了什么",
          to: ["@foreman"],
        },
      } as any,
      "g-demo",
      [],
      { default_send_to: "foreman" } as any,
    );

    expect(state?.toText).toBe("@foreman");
    expect(state?.replyTarget.by).toBe("@foreman");
  });

  it("preserves remote destination routing when replying to a cross-group source message", () => {
    const state = buildReplyComposerState(
      {
        id: "evt-remote-source",
        kind: "chat.message",
        by: "user",
        data: {
          text: "发给远端 foreman 的消息",
          to: ["user"],
          dst_group_id: "g_7e3d34fa5b06",
          dst_to: ["@foreman"],
        },
      } as any,
      "g-local",
      [],
      { default_send_to: "foreman" } as any,
    );

    expect(state?.destGroupId).toBe("g_7e3d34fa5b06");
    expect(state?.toText).toBe("@foreman");
    expect(state?.replyTarget?.remoteDstGroupId).toBe("g_7e3d34fa5b06");
    expect(state?.replyTarget?.remoteDstTo).toEqual(["@foreman"]);
  });

  it("does not treat the local source event id as a remote reply anchor", () => {
    const state = buildReplyComposerState(
      {
        id: "evt-local-source-only",
        kind: "chat.message",
        by: "user",
        data: {
          text: "发给远端 foreman 的消息",
          to: ["user"],
          dst_group_id: "g_remote",
          dst_to: ["@foreman"],
        },
      } as any,
      "g-local",
      [],
      { default_send_to: "foreman" } as any,
    );

    expect(state?.replyTarget?.eventId).toBe("evt-local-source-only");
    expect(state?.replyTarget?.remoteReplyToEventId).toBeUndefined();
  });
});
