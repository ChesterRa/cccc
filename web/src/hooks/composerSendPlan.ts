import type { GroupMeta } from "../types";
import type { ComposerGroupMentionToken } from "./composerGroupMentions";
import { resolveSelectedComposerGroupMentionTargets } from "./composerGroupMentions";

export type ComposerSendPlanTarget = {
  groupId: string;
  isCrossGroup: boolean;
  isRemote?: boolean;
  source: "selected_group" | "group_mention" | "remote_chip";
  recipientTokens?: string[];
};

export function buildComposerSendPlanTargets({
  selectedGroupId,
  dstGroupId,
  isCrossGroup,
  text,
  groupMentionTokens,
  groups,
  remoteGroupIds = [],
  includeSelectedGroup = false,
}: {
  selectedGroupId: string;
  dstGroupId: string;
  isCrossGroup: boolean;
  text: string;
  groupMentionTokens: ComposerGroupMentionToken[];
  groups: GroupMeta[];
  remoteGroupIds?: string[];
  includeSelectedGroup?: boolean;
}): ComposerSendPlanTarget[] {
  const selected = String(selectedGroupId || "").trim();
  const dst = String(dstGroupId || "").trim();
  const groupsById = new Map<string, GroupMeta>();
  for (const group of groups || []) {
    const groupId = String(group.group_id || "").trim();
    if (groupId) groupsById.set(groupId, group);
  }
  const targets: ComposerSendPlanTarget[] = [];
  const addTarget = (target: ComposerSendPlanTarget) => {
    const groupId = String(target.groupId || "").trim();
    if (!groupId) return;
    const existingIndex = targets.findIndex((item) => item.groupId === groupId);
    const normalized = { ...target, groupId };
    if (existingIndex >= 0) {
      if (target.source === "remote_chip") {
        targets[existingIndex] = normalized;
      }
      return;
    }
    targets.push(normalized);
  };

  if (includeSelectedGroup && selected) {
    addTarget({ groupId: selected, isCrossGroup: false, source: "selected_group" });
  }

  if (isCrossGroup && dst && dst !== selected) {
    const group = groupsById.get(dst);
    addTarget({
      groupId: dst,
      isCrossGroup: true,
      isRemote: Boolean(group?.group_bridge_remote),
      source: "selected_group",
    });
    return targets;
  }

  const mentionTargets = resolveSelectedComposerGroupMentionTargets({
    text,
    selectedGroupId: selected,
    groups,
    tokens: groupMentionTokens,
  });
  for (const target of mentionTargets) {
    const group = groupsById.get(String(target.groupId || "").trim());
    addTarget({
      groupId: target.groupId,
      isCrossGroup: true,
      isRemote: Boolean(group?.group_bridge_remote),
      source: "group_mention",
    });
  }

  for (const remoteGroupId of remoteGroupIds || []) {
    const groupId = String(remoteGroupId || "").trim();
    if (!groupId || groupId === selected) continue;
    const group = groupsById.get(groupId);
    if (!group?.group_bridge_remote) continue;
    addTarget({
      groupId,
      isCrossGroup: true,
      isRemote: true,
      source: "remote_chip",
      recipientTokens: ["@foreman"],
    });
  }

  if (!targets.length) {
    return [{ groupId: selected, isCrossGroup: false, source: "selected_group" }];
  }
  return targets;
}
