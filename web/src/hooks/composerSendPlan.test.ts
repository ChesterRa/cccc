import { describe, expect, it } from "vitest";

import type { GroupMeta } from "../types";
import { createComposerGroupMentionToken } from "./composerGroupMentions";
import { buildComposerSendPlanTargets } from "./composerSendPlan";

describe("buildComposerSendPlanTargets", () => {
  const groups = [
    { group_id: "g_local", title: "Local" },
    { group_id: "self-agent", title: "self-agent" },
    { group_id: "print", title: "钉钉打印" },
  ] as GroupMeta[];

  it("plans one cross-group send per selected #group token", () => {
    const text = "当前我们的 #self-agent #钉钉打印 群发解析";
    const first = createComposerGroupMentionToken({ groupId: "self-agent", token: "#self-agent", start: text.indexOf("#self-agent") })!;
    const second = createComposerGroupMentionToken({ groupId: "print", token: "#钉钉打印", start: text.indexOf("#钉钉打印") })!;

    expect(buildComposerSendPlanTargets({
      selectedGroupId: "g_local",
      dstGroupId: "g_local",
      isCrossGroup: false,
      text,
      groupMentionTokens: [first, second],
      groups,
    })).toEqual([
      { groupId: "self-agent", isCrossGroup: true, isRemote: false, source: "group_mention" },
      { groupId: "print", isCrossGroup: true, isRemote: false, source: "group_mention" },
    ]);
  });

  it("falls back to the selected group when no selected #group token is live", () => {
    expect(buildComposerSendPlanTargets({
      selectedGroupId: "g_local",
      dstGroupId: "g_local",
      isCrossGroup: false,
      text: "copied #self-agent text",
      groupMentionTokens: [],
      groups,
    })).toEqual([{ groupId: "g_local", isCrossGroup: false, source: "selected_group" }]);
  });

  it("plans selected remote group chips as remote foreman sends", () => {
    expect(buildComposerSendPlanTargets({
      selectedGroupId: "g_local",
      dstGroupId: "g_local",
      isCrossGroup: false,
      text: "check remote status",
      groupMentionTokens: [],
      groups: [
        ...groups,
        { group_id: "g_remote_a", title: "Remote A", federation_remote: true },
        { group_id: "g_remote_b", title: "Remote B", federation_remote: true },
      ] as GroupMeta[],
      remoteGroupIds: ["g_remote_a", "g_remote_b"],
    })).toEqual([
      { groupId: "g_remote_a", isCrossGroup: true, isRemote: true, source: "remote_chip", recipientTokens: ["@foreman"] },
      { groupId: "g_remote_b", isCrossGroup: true, isRemote: true, source: "remote_chip", recipientTokens: ["@foreman"] },
    ]);
  });

  it("keeps an explicit local recipient when remote chips are selected", () => {
    expect(buildComposerSendPlanTargets({
      selectedGroupId: "g_local",
      dstGroupId: "g_local",
      isCrossGroup: false,
      text: "also tell local foreman",
      groupMentionTokens: [],
      groups: [
        ...groups,
        { group_id: "g_remote_a", title: "Remote A", federation_remote: true },
      ] as GroupMeta[],
      remoteGroupIds: ["g_remote_a"],
      includeSelectedGroup: true,
    })).toEqual([
      { groupId: "g_local", isCrossGroup: false, source: "selected_group" },
      { groupId: "g_remote_a", isCrossGroup: true, isRemote: true, source: "remote_chip", recipientTokens: ["@foreman"] },
    ]);
  });
});
