import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "useChatTab.ts"), "utf8");

describe("useChatTab request triggers", () => {
  it("does not refresh slash commands from chat ledger event changes", () => {
    expect(source).not.toContain("latestFormalChatEventKey");
    expect(source).not.toMatch(/latestFormalChatEventKey[\s\S]*refreshSlashCommands/);
  });

  it("delegates message-body mention suggestions to the focused builder", () => {
    expect(source).not.toContain("buildGroupMentionSuggestions");
    expect(source).toContain("buildComposerMentionSuggestions");
    expect(source).toMatch(/const mentionSuggestions = useMemo\(\(\) => \{[\s\S]*return buildComposerMentionSuggestions\(\{/);
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

  it("keeps cross-group sends aligned with the composer recipient snapshot", () => {
    expect(source).toContain("const crossToTokensSnapshot = buildComposerSendRecipientTokens({");
    expect(source).toContain("const targetTo = Array.isArray(target.recipientTokens)");
    expect(source).toContain('target.isRemote\n                ? ["@foreman"]');
    expect(source).not.toContain('const to = isCrossGroup ? ["@foreman"] : toTokensSnapshot;');
  });

  it("treats remote group chips as cross-group for slash command guards", () => {
    expect(source).toContain("const slashGuardSendGroupId = sendsCrossGroup");
    expect(source).toContain("sendGroupId: slashGuardSendGroupId");
    expect(source).not.toContain("sendGroupId: dstGroup,\n    }))");
  });

  it("does not restore the full composer after a partial multi-target cross-group send", () => {
    expect(source).toContain("let successfulSendCount = 0;");
    expect(source).toMatch(/resp = await api\.sendCrossGroupMessage[\s\S]*if \(!resp\.ok\) break;[\s\S]*successfulSendCount \+= 1;/);
    expect(source).toContain("const shouldRestoreComposer = successfulSendCount === 0;");
    expect(source).toContain("if (shouldRestoreComposer) restoreComposerState();");
  });

  it("allows attachment sends to remote group chips while blocking local cross-group attachments", () => {
    expect(source).toContain("shouldBlockLocalCrossGroupAttachments({");
    expect(source).toContain("Local cross-group send does not support attachments yet.");
    expect(source).toContain("composerFilesSnapshot.length > 0 ? composerFilesSnapshot : undefined");
    expect(source).not.toContain("Cross-group send does not support attachments yet.");
  });
});
