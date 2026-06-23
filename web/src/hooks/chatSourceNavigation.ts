import type { GroupMeta } from "../types";

export function canOpenSourceMessageLocally(groups: GroupMeta[], srcGroupId: string): boolean {
  const gid = String(srcGroupId || "").trim();
  if (!gid) return false;
  return (groups || []).some((group) => {
    if (String(group?.group_id || "").trim() !== gid) return false;
    return !group.group_bridge_remote;
  });
}
