import type { GroupMeta } from "../types";
import type { ComposerGroupMentionToken } from "./composerGroupMentions";
import { resolveSelectedComposerGroupMentionTargets } from "./composerGroupMentions";

export type ComposerSendPlanTarget = {
  groupId: string;
  isCrossGroup: boolean;
  source: "selected_group" | "group_mention";
  recipientTokens?: string[];
};

export function buildComposerSendPlanTargets({
  selectedGroupId,
  dstGroupId,
  isCrossGroup,
  text,
  groupMentionTokens,
  groups,
}: {
  selectedGroupId: string;
  dstGroupId: string;
  isCrossGroup: boolean;
  text: string;
  groupMentionTokens: ComposerGroupMentionToken[];
  groups: GroupMeta[];
}): ComposerSendPlanTarget[] {
  const selected = String(selectedGroupId || "").trim();
  const dst = String(dstGroupId || "").trim();
  const targets: ComposerSendPlanTarget[] = [];
  const addTarget = (target: ComposerSendPlanTarget) => {
    const groupId = String(target.groupId || "").trim();
    if (!groupId) return;
    const existingIndex = targets.findIndex((item) => item.groupId === groupId);
    const normalized = { ...target, groupId };
    if (existingIndex >= 0) {
      return;
    }
    targets.push(normalized);
  };

  if (isCrossGroup && dst && dst !== selected) {
    addTarget({ groupId: dst, isCrossGroup: true, source: "selected_group" });
    return targets;
  }

  const mentionTargets = resolveSelectedComposerGroupMentionTargets({
    text,
    selectedGroupId: selected,
    groups,
    tokens: groupMentionTokens,
  });
  if (mentionTargets.length > 0 && selected) {
    addTarget({ groupId: selected, isCrossGroup: false, source: "selected_group" });
  }

  if (!targets.length) {
    return [{ groupId: selected, isCrossGroup: false, source: "selected_group" }];
  }
  return targets;
}
