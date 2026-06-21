import type { GroupMeta } from "../types";
import type { ComposerGroupMentionToken } from "./composerGroupMentions";
import { resolveSelectedComposerGroupMentionTargets } from "./composerGroupMentions";

export type ComposerSendPlanTarget = {
  groupId: string;
  isCrossGroup: boolean;
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
  if (isCrossGroup && dst && dst !== selected) {
    return [{ groupId: dst, isCrossGroup: true }];
  }

  const targets = resolveSelectedComposerGroupMentionTargets({
    text,
    selectedGroupId: selected,
    groups,
    tokens: groupMentionTokens,
  });
  if (!targets.length) {
    return [{ groupId: selected, isCrossGroup: false }];
  }
  return targets.map((target) => ({ groupId: target.groupId, isCrossGroup: true }));
}
