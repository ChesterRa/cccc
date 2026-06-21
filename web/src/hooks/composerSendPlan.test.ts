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
      { groupId: "self-agent", isCrossGroup: true },
      { groupId: "print", isCrossGroup: true },
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
    })).toEqual([{ groupId: "g_local", isCrossGroup: false }]);
  });
});
