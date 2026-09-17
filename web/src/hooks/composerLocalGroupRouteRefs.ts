import type { GroupMeta, LocalGroupRouteMessageRef, ConnectGroupMessageRef } from "../types";
import type { ComposerGroupMentionToken } from "./composerGroupMentions";
import {
  pruneComposerGroupMentionTokens,
  resolveSelectedComposerGroupMentionTargets,
} from "./composerGroupMentions";

export function buildComposerConnectGroupRefs(
  text: string,
  tokens: ComposerGroupMentionToken[],
): ConnectGroupMessageRef[] {
  const seen = new Set<string>();
  return pruneComposerGroupMentionTokens({ text, tokens }).flatMap((token) => {
    const remote = token.remote;
    if (!remote) return [];
    const key = JSON.stringify([remote.instance_id, remote.group_id]);
    if (seen.has(key)) return [];
    seen.add(key);
    return [
      {
        kind: "connect_group_ref",
        instance_id: remote.instance_id,
        instance_name: remote.instance_name,
        group_id: remote.group_id,
        group_title: remote.title,
        token: token.token,
      },
    ];
  });
}

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
    if (!group) return [];
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
