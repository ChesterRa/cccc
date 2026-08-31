import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import * as api from "../services/api";
import { shouldBlockLocalCrossGroupAttachments } from "../utils/chatSend";
import {
  buildComposerSendRecipientTokens,
  shouldRestoreComposerAfterFailedSend,
} from "./chat/chatComposerState";
import { dispatchPreparedMessage } from "./chat/chatMessageSend";

vi.mock("../services/api", () => ({
  sendMessage: vi.fn(),
  replyMessage: vi.fn(),
  sendCrossGroupMessage: vi.fn(),
}));

describe("useChatTab request triggers", () => {
  beforeEach(() => vi.clearAllMocks());

  it("keeps cross-group sends aligned with the composer recipient snapshot", async () => {
    const crossTo = buildComposerSendRecipientTokens({
      toText: "remote-agent, local-only",
      isCrossGroup: true,
      validRecipientSet: new Set(["remote-agent", "local-only"]),
      crossGroupValidRecipientSet: new Set(["remote-agent"]),
    });
    vi.mocked(api.sendCrossGroupMessage).mockResolvedValue({ ok: true, result: {} });

    await dispatchPreparedMessage({
      selectedGroupId: "g_local",
      text: "hello",
      localTo: ["local-only"],
      crossTo,
      files: [],
      messageMode: "send",
      localId: "local_1",
      refs: [],
      replyTarget: null,
      remoteReplyGroupId: "",
      remoteReplyTo: [],
      sendPlanTargets: [{ groupId: "g_remote", isCrossGroup: true, source: "selected_group" }],
      sendsCrossGroup: true,
    });

    expect(api.sendCrossGroupMessage).toHaveBeenCalledWith(
      "g_local",
      "g_remote",
      "hello",
      ["remote-agent"],
      "send",
      undefined,
    );
  });

  it("can fulfill a reply through Mail without requesting an immediate prompt", async () => {
    vi.mocked(api.replyMessage).mockResolvedValue({ ok: true, result: {} });

    const result = await dispatchPreparedMessage({
      selectedGroupId: "g_local",
      text: "reply later",
      localTo: ["user"],
      crossTo: [],
      files: [],
      messageMode: "mail",
      localId: "local_reply_1",
      refs: [],
      replyTarget: { eventId: "event-1", by: "user", text: "question" },
      remoteReplyGroupId: "",
      remoteReplyTo: [],
      sendPlanTargets: [{ groupId: "g_local", isCrossGroup: false, source: "selected_group" }],
      sendsCrossGroup: false,
    });

    expect(result.successfulSendCount).toBe(1);
    expect(api.replyMessage).toHaveBeenCalledWith(
      "g_local",
      "reply later",
      ["user"],
      "event-1",
      undefined,
      "local_reply_1",
      [],
      "question",
      "mail",
    );
  });

  it("does not restore the full composer after a partial multi-target cross-group send", async () => {
    vi.mocked(api.sendCrossGroupMessage)
      .mockResolvedValueOnce({ ok: true, result: {} })
      .mockResolvedValueOnce({ ok: false, error: { code: "failed", message: "failed" } });
    const result = await dispatchPreparedMessage({
      selectedGroupId: "g_local",
      text: "hello",
      localTo: [],
      crossTo: ["@foreman"],
      files: [],
      messageMode: "send",
      localId: "local_1",
      refs: [],
      replyTarget: null,
      remoteReplyGroupId: "",
      remoteReplyTo: [],
      sendPlanTargets: ["g_one", "g_two"].map((groupId) => ({
        groupId,
        isCrossGroup: true,
        source: "selected_group" as const,
      })),
      sendsCrossGroup: true,
    });

    expect(result.successfulSendCount).toBe(1);
    expect(result.response.ok).toBe(false);
    expect(shouldRestoreComposerAfterFailedSend(result.successfulSendCount)).toBe(false);
    expect(shouldRestoreComposerAfterFailedSend(0)).toBe(true);
  });

  it("allows attachment sends to remote group chips while blocking local cross-group attachments", async () => {
    const file = new File(["payload"], "payload.txt", { type: "text/plain" });
    expect(
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: 1,
        targets: [{ isCrossGroup: true, isRemote: false }],
      }),
    ).toBe(true);
    expect(
      shouldBlockLocalCrossGroupAttachments({
        attachmentCount: 1,
        targets: [{ isCrossGroup: true, isRemote: true }],
      }),
    ).toBe(false);
    vi.mocked(api.sendCrossGroupMessage).mockResolvedValue({ ok: true, result: {} });

    await dispatchPreparedMessage({
      selectedGroupId: "g_local",
      text: "hello",
      localTo: [],
      crossTo: [],
      files: [file],
      messageMode: "send",
      localId: "local_1",
      refs: [],
      replyTarget: null,
      remoteReplyGroupId: "",
      remoteReplyTo: [],
      sendPlanTargets: [
        {
          groupId: "g_remote",
          isCrossGroup: true,
          isRemote: true,
          source: "remote_chip",
          recipientTokens: ["@foreman"],
        },
      ],
      sendsCrossGroup: true,
    });

    expect(api.sendCrossGroupMessage).toHaveBeenCalledWith(
      "g_local",
      "g_remote",
      "hello",
      ["@foreman"],
      "send",
      [file],
    );
  });
});
