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

  it("uses composer destination group state for route chips", () => {
    expect(source).toContain("destGroupId: composerStateSnapshot.destGroupId");
    expect(source).not.toContain("destGroupId: latestSelectedGroupId");
  });

  it("keeps cross-group sends aligned with the composer recipient snapshot", () => {
    expect(source).toContain("const to = toTokensSnapshot;");
    expect(source).not.toContain('const to = isCrossGroup ? ["@foreman"] : toTokensSnapshot;');
  });
});
