import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

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

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "useChatTab.ts"), "utf8");

describe("useChatTab request triggers", () => {
  beforeEach(() => vi.clearAllMocks());

  it("does not refresh slash commands from chat ledger event changes", () => {
    expect(source).not.toContain("latestFormalChatEventKey");
    expect(source).not.toMatch(/latestFormalChatEventKey[\s\S]*refreshSlashCommands/);
  });

  it("delegates message-body mention suggestions to the focused builder", () => {
    expect(source).not.toContain("buildGroupMentionSuggestions");
    expect(source).toContain("buildComposerMentionSuggestions");
    expect(source).toMatch(
      /const mentionSuggestions = useMemo\(\(\) => \{[\s\S]*return buildComposerMentionSuggestions\(\{/,
    );
    expect(source).toContain("kind: mentionKind");
    expect(source).toContain("filter: mentionFilter");
    expect(source).toContain('mentionActorScope === "selected" ? actors : recipientActors');
    expect(source).toContain("recipientActors");
    expect(source).toContain("groups");
  });

  it("does not promote selected @ mentions into recipient state", () => {
    expect(source).not.toContain("appendRecipientToken");
    expect(source).not.toContain("pruneMissingMentionRecipientTokens");
    expect(source).toContain("Message-body mentions are text helpers");
  });

  it("uses composer destination group state for route chips", () => {
    expect(source).toContain("destGroupId: composerStateSnapshot.destGroupId");
    expect(source).not.toContain("destGroupId: latestSelectedGroupId");
  });

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
      priority: "normal",
      replyRequired: false,
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
      "normal",
      false,
      undefined,
    );
  });

  it("treats remote group chips as cross-group for slash command guards", () => {
    expect(source).toContain("const slashGuardSendGroupId = sendsCrossGroup");
    expect(source).toContain("sendGroupId: slashGuardSendGroupId");
    expect(source).not.toContain("sendGroupId: dstGroup,\n    }))");
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
      priority: "normal",
      replyRequired: false,
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
      priority: "normal",
      replyRequired: false,
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
      "normal",
      false,
      [file],
    );
  });
});
