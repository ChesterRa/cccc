import type { GroupMeta } from "../types";
import type { ConnectMentionGroup } from "./useConnectMentionGroups";

export interface ComposerGroupMentionToken {
  groupId: string;
  token: string;
  start: number;
  end: number;
  remote?: ConnectMentionGroup;
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
  ].filter(
    (value, index, list): value is string => Boolean(value) && list.indexOf(value) === index,
  );
}

function isTokenBoundary(ch: string): boolean {
  return !ch || /\s|[,，。.!?;；:：]/.test(ch);
}

export function createComposerGroupMentionToken({
  groupId,
  token,
  start,
  remote,
}: {
  groupId: string;
  token: string;
  start: number;
  remote?: ConnectMentionGroup;
}): ComposerGroupMentionToken | null {
  const cleanGroupId = String(groupId || "").trim();
  const cleanToken = cleanTokenText(token);
  const safeStart = Number.isFinite(start) ? Math.max(0, Math.floor(start)) : 0;
  if (!cleanGroupId || !cleanToken) return null;
  return {
    groupId: cleanGroupId,
    token: cleanToken,
    start: safeStart,
    end: safeStart + cleanToken.length,
    ...(remote ? { remote } : {}),
  };
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

// Drop tokens whose text range no longer matches the composer text.
//
// Returns the input array itself when nothing was dropped: callers feed the
// result straight into React state, and a fresh array on every keystroke
// would re-render the whole chat tab for a change that is not one.
function keepLiveMentionTokens<T extends { start: number; end: number; token: string }>(
  text: string,
  tokens: T[],
): T[] {
  const source = String(text || "");
  const input = tokens || [];
  const live = input.filter((token) => {
    const start = Number.isFinite(token.start) ? Math.max(0, Math.floor(token.start)) : -1;
    const end = Number.isFinite(token.end) ? Math.max(start, Math.floor(token.end)) : -1;
    if (start < 0 || end <= start || end > source.length) return false;
    return (
      source.slice(start, end) === token.token &&
      isTokenBoundary(source[start - 1] || "") &&
      isTokenBoundary(source[end] || "")
    );
  });
  return live.length === input.length ? input : live;
}

export function pruneComposerGroupMentionTokens({
  text,
  tokens,
}: {
  text: string;
  tokens: ComposerGroupMentionToken[];
}): ComposerGroupMentionToken[] {
  return keepLiveMentionTokens(text, tokens);
}

export function pruneComposerAgentMentionTokens({
  text,
  tokens,
}: {
  text: string;
  tokens: ComposerAgentMentionToken[];
}): ComposerAgentMentionToken[] {
  return keepLiveMentionTokens(text, tokens);
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
    if (token.remote) continue;
    if (!token.groupId || token.groupId === selected) continue;
    const group = (groups || []).find(
      (item) => String(item.group_id || "").trim() === token.groupId,
    );
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
    if (token.remote) continue;
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

export function resolveControlledComposerMentionContext({
  text,
  atIndex,
  tokens,
}: {
  text: string;
  atIndex: number;
  tokens: ComposerGroupMentionToken[];
}): {
  scope: "selected" | "destination";
  mentionTargetGroupId: string;
  remote?: ConnectMentionGroup;
} {
  const source = String(text || "");
  const safeAt = Number.isFinite(atIndex) ? Math.max(0, Math.floor(atIndex)) : 0;
  const segStartNl = source.lastIndexOf("\n", Math.max(0, safeAt - 1));
  const segStart = segStartNl >= 0 ? segStartNl + 1 : 0;
  const liveTokens = pruneComposerGroupMentionTokens({ text: source, tokens });
  const best = liveTokens
    .filter((token) => token.start >= segStart && token.end <= safeAt)
    .sort((a, b) => b.start - a.start)[0];
  if (!best) return { scope: "selected", mentionTargetGroupId: "" };
  if (best.remote) return { scope: "destination", mentionTargetGroupId: "", remote: best.remote };
  return { scope: "destination", mentionTargetGroupId: best.groupId };
}
