import type { GroupMeta, LocalGroupRouteMessageRef } from "../types";
import type { ComposerGroupMentionToken } from "./composerGroupMentions";
import { resolveSelectedComposerGroupMentionTargets } from "./composerGroupMentions";

export function buildComposerLocalGroupRouteRefs({
  text,
  selectedGroupId,
  tokens,
  groups,
}: {
  text: string;
  selectedGroupId: string;
  tokens: ComposerGroupMentionToken[];
  groups: GroupMeta[];
}): LocalGroupRouteMessageRef[] {
  const targets = resolveSelectedComposerGroupMentionTargets({
    text,
    selectedGroupId,
    groups,
    tokens,
  });
  const groupsById = new Map(
    (groups || []).map((group) => [String(group.group_id || "").trim(), group]),
  );

  return targets.flatMap((token) => {
    const group = groupsById.get(token.groupId);
    if (!group || group.group_bridge_remote) return [];
    const title = String(group.title || "").trim() || String(group.topic || "").trim();
    return [
      {
        kind: "local_group_route",
        group_id: token.groupId,
        group_title: title || undefined,
        token: token.token,
      },
    ];
  });
}
