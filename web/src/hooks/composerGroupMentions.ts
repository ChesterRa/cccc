import type { GroupBridgeRouteMessageRef, GroupMeta } from "../types";

export interface ComposerGroupMentionToken {
  groupId: string;
  token: string;
  start: number;
  end: number;
}

export interface ComposerAgentMentionToken {
  actorId: string;
  token: string;
  start: number;
  end: number;
  scope: "selected" | "destination";
}

function cleanTokenText(value: string): string {
  return String(value || "").trim();
}

function groupCandidateTokens(group: GroupMeta): string[] {
  return [
    String(group.group_id || "").trim(),
    String(group.title || "").trim(),
    String(group.topic || "").trim(),
  ].filter((value, index, list): value is string => Boolean(value) && list.indexOf(value) === index);
}

function isTokenBoundary(ch: string): boolean {
  return !ch || /\s|[,，。.!?;；:：]/.test(ch);
}

export function createComposerGroupMentionToken({
  groupId,
  token,
  start,
}: {
  groupId: string;
  token: string;
  start: number;
}): ComposerGroupMentionToken | null {
  const cleanGroupId = String(groupId || "").trim();
  const cleanToken = cleanTokenText(token);
  const safeStart = Number.isFinite(start) ? Math.max(0, Math.floor(start)) : 0;
  if (!cleanGroupId || !cleanToken) return null;
  return { groupId: cleanGroupId, token: cleanToken, start: safeStart, end: safeStart + cleanToken.length };
}

export function createComposerAgentMentionToken({
  actorId,
  token,
  start,
  scope,
}: {
  actorId: string;
  token: string;
  start: number;
  scope: "selected" | "destination";
}): ComposerAgentMentionToken | null {
  const cleanActorId = String(actorId || "").trim();
  const cleanToken = cleanTokenText(token);
  const safeStart = Number.isFinite(start) ? Math.max(0, Math.floor(start)) : 0;
  if (!cleanActorId || !cleanToken) return null;
  return {
    actorId: cleanActorId,
    token: cleanToken,
    start: safeStart,
    end: safeStart + cleanToken.length,
    scope,
  };
}

export function pruneComposerGroupMentionTokens({
  text,
  tokens,
}: {
  text: string;
  tokens: ComposerGroupMentionToken[];
}): ComposerGroupMentionToken[] {
  const source = String(text || "");
  return (tokens || []).filter((token) => {
    const start = Number.isFinite(token.start) ? Math.max(0, Math.floor(token.start)) : -1;
    const end = Number.isFinite(token.end) ? Math.max(start, Math.floor(token.end)) : -1;
    if (start < 0 || end <= start || end > source.length) return false;
    return source.slice(start, end) === token.token && isTokenBoundary(source[start - 1] || "") && isTokenBoundary(source[end] || "");
  });
}

export function pruneComposerAgentMentionTokens({
  text,
  tokens,
}: {
  text: string;
  tokens: ComposerAgentMentionToken[];
}): ComposerAgentMentionToken[] {
  const source = String(text || "");
  return (tokens || []).filter((token) => {
    const start = Number.isFinite(token.start) ? Math.max(0, Math.floor(token.start)) : -1;
    const end = Number.isFinite(token.end) ? Math.max(start, Math.floor(token.end)) : -1;
    if (start < 0 || end <= start || end > source.length) return false;
    return source.slice(start, end) === token.token && isTokenBoundary(source[start - 1] || "") && isTokenBoundary(source[end] || "");
  });
}

export function resolveSelectedComposerGroupMention({
  text,
  selectedGroupId,
  groups,
  tokens,
}: {
  text: string;
  selectedGroupId: string;
  groups: GroupMeta[];
  tokens: ComposerGroupMentionToken[];
}): ComposerGroupMentionToken | null {
  const selected = String(selectedGroupId || "").trim();
  const liveTokens = pruneComposerGroupMentionTokens({ text, tokens });
  let best: ComposerGroupMentionToken | null = null;
  for (const token of liveTokens) {
    if (!token.groupId || token.groupId === selected) continue;
    const group = (groups || []).find((item) => String(item.group_id || "").trim() === token.groupId);
    if (!group) continue;
    if (!groupCandidateTokens(group).includes(token.token.replace(/^#/, ""))) continue;
    if (!best || token.start >= best.start) best = token;
  }
  return best;
}

export function resolveSelectedComposerGroupMentionTargets({
  text,
  selectedGroupId,
  groups,
  tokens,
}: {
  text: string;
  selectedGroupId: string;
  groups: GroupMeta[];
  tokens: ComposerGroupMentionToken[];
}): ComposerGroupMentionToken[] {
  const selected = String(selectedGroupId || "").trim();
  const liveTokens = pruneComposerGroupMentionTokens({ text, tokens });
  const seen = new Set<string>();
  const out: ComposerGroupMentionToken[] = [];
  const groupsById = new Map<string, GroupMeta>();
  for (const group of groups || []) {
    const groupId = String(group.group_id || "").trim();
    if (groupId) groupsById.set(groupId, group);
  }

  for (const token of [...liveTokens].sort((a, b) => a.start - b.start)) {
    const groupId = String(token.groupId || "").trim();
    if (!groupId || groupId === selected || seen.has(groupId)) continue;
    const group = groupsById.get(groupId);
    if (!group) continue;
    if (!groupCandidateTokens(group).includes(token.token.replace(/^#/, ""))) continue;
    seen.add(groupId);
    out.push(token);
  }
  return out;
}

export function buildComposerGroupBridgeRouteRefs({
  text,
  tokens,
  groups,
}: {
  text: string;
  tokens: ComposerGroupMentionToken[];
  groups: GroupMeta[];
}): GroupBridgeRouteMessageRef[] {
  const liveTokens = pruneComposerGroupMentionTokens({ text, tokens });
  const refs: GroupBridgeRouteMessageRef[] = [];
  const seen = new Set<string>();

  for (const token of liveTokens) {
    const groupId = String(token.groupId || "").trim();
    if (!groupId || seen.has(groupId)) continue;
    const group = (groups || []).find((item) => String(item.group_id || "").trim() === groupId);
    if (!group?.group_bridge_remote) continue;
    seen.add(groupId);
    refs.push({
      kind: "group_bridge_route",
      local_group_id: String(group.group_bridge_local_group_id || "").trim() || undefined,
      remote_group_id: groupId,
      remote_group_title: String(group.title || "").trim(),
      remote_endpoint: String(group.group_bridge_remote_endpoint || "").trim(),
      remote_peer_id: String(group.group_bridge_remote_peer_id || "").trim(),
      trust_id: String(group.group_bridge_trust_id || "").trim(),
      token: token.token,
    });
  }

  return refs;
}

export function resolveControlledComposerMentionContext({
  text,
  atIndex,
  tokens,
}: {
  text: string;
  atIndex: number;
  tokens: ComposerGroupMentionToken[];
}): { scope: "selected" | "destination"; mentionTargetGroupId: string } {
  const source = String(text || "");
  const safeAt = Number.isFinite(atIndex) ? Math.max(0, Math.floor(atIndex)) : 0;
  const segStartNl = source.lastIndexOf("\n", Math.max(0, safeAt - 1));
  const segStart = segStartNl >= 0 ? segStartNl + 1 : 0;
  const liveTokens = pruneComposerGroupMentionTokens({ text: source, tokens });
  const best = liveTokens
    .filter((token) => token.start >= segStart && token.end <= safeAt)
    .sort((a, b) => b.start - a.start)[0];
  if (!best) return { scope: "selected", mentionTargetGroupId: "" };
  return { scope: "destination", mentionTargetGroupId: best.groupId };
}

export function extractControlledGroupMentionTargetActor({
  text,
  token,
  agentTokens = [],
}: {
  text: string;
  token: ComposerGroupMentionToken | null;
  agentTokens?: ComposerAgentMentionToken[];
}): string {
  if (!token) return "";
  const source = String(text || "");
  const live = pruneComposerGroupMentionTokens({ text: source, tokens: [token] })[0] || null;
  if (!live) return "";
  const segEndNl = source.indexOf("\n", live.end);
  const segEnd = segEndNl >= 0 ? segEndNl : source.length;
  const liveAgents = pruneComposerAgentMentionTokens({ text: source, tokens: agentTokens });
  const target = liveAgents
    .filter((agent) => agent.scope === "destination" && agent.start >= live.end && agent.end <= segEnd)
    .sort((a, b) => a.start - b.start)[0];
  return target?.actorId || "";
}
