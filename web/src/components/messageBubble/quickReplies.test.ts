import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const replyMessage = vi.hoisted(() => vi.fn());
vi.mock("../../services/api", () => ({ replyMessage }));

import { sendQuickReply, type QuickReplyTarget } from "./quickReplies";

const target: QuickReplyTarget = {
  groupId: "g",
  replyEventId: "event-1",
  to: ["worker"],
  quoteText: "Plan ready",
};
const clientIdOf = (call: number) => replyMessage.mock.calls[call][5] as string;

describe("sendQuickReply", () => {
  beforeEach(() => replyMessage.mockReset());

  it("replies to the author with the ready text, and starts over once it is sent", async () => {
    replyMessage.mockResolvedValue({ ok: true, result: { event: { id: "reply-1" } } });
    expect(await sendQuickReply(target, "proceed", "Go ahead.")).toEqual({ status: "sent" });
    expect(replyMessage).toHaveBeenCalledWith(
      "g",
      "Go ahead.",
      ["worker"],
      "event-1",
      undefined,
      expect.any(String),
      [],
      "Plan ready",
      "send",
    );
    await sendQuickReply(target, "proceed", "Go ahead.");
    // Sending it again on purpose is a new message.
    expect(clientIdOf(1)).not.toBe(clientIdOf(0));
  });

  it("repeats an unconfirmed request under the same client ID", async () => {
    replyMessage.mockResolvedValueOnce({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "Network request failed" },
    });
    expect(await sendQuickReply(target, "why", "Why?")).toEqual({
      status: "failed",
      message: "Network request failed",
      unknown: true,
    });
    // The daemon may have recorded a request whose answer it dropped.
    replyMessage.mockResolvedValueOnce({
      ok: false,
      error: { code: "daemon_unavailable", message: "CCCC daemon unavailable" },
    });
    expect(await sendQuickReply(target, "why", "Why?")).toEqual({
      status: "failed",
      message: "CCCC daemon unavailable",
      unknown: true,
    });
    replyMessage.mockResolvedValueOnce({
      ok: false,
      error: { code: "group_not_found", message: "group not found" },
    });
    expect(await sendQuickReply(target, "why", "Why?")).toEqual({
      status: "failed",
      message: "group not found",
      unknown: false,
    });
    // A success with no recorded event proves nothing either.
    replyMessage.mockResolvedValueOnce({ ok: true, result: {} });
    expect(await sendQuickReply(target, "why", "Why?")).toEqual({
      status: "failed",
      message: "",
      unknown: true,
    });
    replyMessage.mockResolvedValueOnce({ ok: true, result: { event: { id: "reply-1" } } });
    expect(await sendQuickReply(target, "why", "Why?")).toEqual({ status: "sent" });
    for (const call of [1, 2, 3, 4]) expect(clientIdOf(call)).toBe(clientIdOf(0));
    // Another reply to the same message is another request.
    replyMessage.mockResolvedValueOnce({ ok: true, result: { event: { id: "reply-2" } } });
    await sendQuickReply(target, "rethink", "Rethink.");
    expect(clientIdOf(5)).not.toBe(clientIdOf(0));
  });

  it("ignores the same quick reply chosen again while it is on its way", async () => {
    let finish: (value: unknown) => void = () => {};
    replyMessage.mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));
    const first = sendQuickReply(target, "choices", "Give me choices.");
    expect(await sendQuickReply(target, "choices", "Give me choices.")).toEqual({ status: "busy" });
    finish({ ok: true, result: { event: { id: "reply-1" } } });
    expect(await first).toEqual({ status: "sent" });
    expect(replyMessage).toHaveBeenCalledOnce();
  });
});
