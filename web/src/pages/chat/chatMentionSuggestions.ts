import type { Actor, GroupMeta } from "../../types";

export type ComposerMentionKind = "agent" | "group";

export type ComposerMentionSuggestion = {
  kind: ComposerMentionKind;
  value: string;
  label: string;
  description?: string;
  meta?: string;
  keywords?: string[];
};

function matchesMentionFilter(item: ComposerMentionSuggestion, needle: string): boolean {
  if (!needle) return true;
  return [item.value, item.label, item.description, item.meta, ...(item.keywords || [])]
    .filter(Boolean)
    .some((part) => String(part).toLowerCase().includes(needle));
}

function buildAgentMentionSuggestions(recipientActors: Actor[], needle: string): ComposerMentionSuggestion[] {
  const base = ["@all", "@foreman", "@peers"];
  const actorItems = recipientActors
    .map((actor) => {
      const id = String(actor.id || "").trim();
      if (!id) return null;
      const title = String(actor.title || "").trim();
      return {
        kind: "agent" as const,
        value: id,
        label: title || id,
        description: title && title !== id ? id : undefined,
        keywords: [id, title].filter(Boolean),
      };
    })
    .filter((item): item is NonNullable<typeof item> => Boolean(item));

  return [
    ...base.map((token) => ({
      kind: "agent" as const,
      value: token,
      label: token,
      description: undefined,
      keywords: [token],
    })),
    ...actorItems,
  ].filter((item) => matchesMentionFilter(item, needle));
}

function buildGroupMentionSuggestions(groups: GroupMeta[], needle: string): ComposerMentionSuggestion[] {
  return (groups || [])
    .filter((group) => String(group.group_id || "").trim())
    .map((group) => {
      const groupId = String(group.group_id || "").trim();
      const title = String(group.title || "").trim();
      const topic = String(group.topic || "").trim();
      const label = title || topic || groupId;
      return {
        kind: "group" as const,
        value: groupId,
        label,
        description: topic || undefined,
        meta: label !== groupId ? groupId : undefined,
        keywords: [groupId, title, topic].filter(Boolean),
      };
    })
    .filter((item) => matchesMentionFilter(item, needle));
}

function containsRouteToken(text: string, token: string): boolean {
  const route = `#${token}`;
  let index = text.indexOf(route);
  while (index >= 0) {
    const before = index === 0 ? "" : text[index - 1];
    const afterIndex = index + route.length;
    const after = afterIndex >= text.length ? "" : text[afterIndex];
    const startsOnBoundary = !before || before === " " || before === "\n";
    const endsOnBoundary = !after || after === " " || after === "\n";
    if (startsOnBoundary && endsOnBoundary) return true;
    index = text.indexOf(route, index + 1);
  }
  return false;
}

function getRouteTokenOccurrences(text: string, token: string): number[] {
  const route = `#${token}`;
  const out: number[] = [];
  let index = text.indexOf(route);
  while (index >= 0) {
    const before = index === 0 ? "" : text[index - 1];
    const afterIndex = index + route.length;
    const after = afterIndex >= text.length ? "" : text[afterIndex];
    const startsOnBoundary = !before || before === " " || before === "\n";
    const endsOnBoundary = !after || after === " " || after === "\n";
    if (startsOnBoundary && endsOnBoundary) out.push(index);
    index = text.indexOf(route, index + 1);
  }
  return out;
}

function getGroupRouteCandidates(group: GroupMeta): string[] {
  return [
    String(group.group_id || "").trim(),
    String(group.title || "").trim(),
    String(group.topic || "").trim(),
  ].filter((value, index, list): value is string => Boolean(value) && list.indexOf(value) === index);
}

export function hasComposerGroupRouteToken({
  text,
  destGroupId,
  selectedGroupId,
  groups,
}: {
  text: string;
  destGroupId: string;
  selectedGroupId: string;
  groups: GroupMeta[];
}): boolean {
  const dest = String(destGroupId || "").trim();
  const selected = String(selectedGroupId || "").trim();
  if (!dest || dest === selected) return true;

  const group = (groups || []).find((item) => String(item.group_id || "").trim() === dest);
  const candidates = group ? getGroupRouteCandidates(group) : [dest];

  return candidates.some((candidate) => containsRouteToken(String(text || ""), candidate));
}

export function getComposerGroupRouteDestination({
  text,
  selectedGroupId,
  groups,
}: {
  text: string;
  selectedGroupId: string;
  groups: GroupMeta[];
}): string {
  const selected = String(selectedGroupId || "").trim();
  const source = String(text || "");
  let best: { index: number; groupId: string } | null = null;

  for (const group of groups || []) {
    const groupId = String(group.group_id || "").trim();
    if (!groupId) continue;
    for (const candidate of getGroupRouteCandidates(group)) {
      for (const index of getRouteTokenOccurrences(source, candidate)) {
        if (!best || index >= best.index) {
          best = { index, groupId };
        }
      }
    }
  }

  return best?.groupId || selected;
}

export interface ComposerHashRouting {
  /** Destination group for the actual send. Always the local group: a user's
   *  `#<group>` token is a local-group agent delegation hint, never a direct
   *  cross-group route. */
  destGroupId: string;
  /** The group referenced by the `#` token, kept as delegation context for the
   *  local agent (empty when the token matches no real group). */
  delegationGroupId: string;
}

// Resolve how a user's `#<group>` token should route. Policy: never set a
// cross-group destination from composer input — the message stays in the local
// group so its agent can decide how to contact the referenced group. The
// referenced group id is surfaced separately as delegation context.
export function resolveComposerHashRouting({
  text,
  selectedGroupId,
  groups,
}: {
  text: string;
  selectedGroupId: string;
  groups: GroupMeta[];
}): ComposerHashRouting {
  const selected = String(selectedGroupId || "").trim();
  const matched = getComposerGroupRouteDestination({ text, selectedGroupId, groups });
  const delegationGroupId = matched && matched !== selected ? matched : "";
  return { destGroupId: selected, delegationGroupId };
}

// A `#group` token only governs `@` tokens in the same "segment" — i.e. the
// same line, with no newline between them. Sending clears the composer, so a
// prior turn never leaks in. This prevents an arbitrary/historical `#` from
// polluting a later bare `@`.
function _segmentStart(text: string, index: number): number {
  const nl = text.lastIndexOf("\n", Math.max(0, index - 1));
  return nl >= 0 ? nl + 1 : 0;
}

function _latestValidHashInRange(
  text: string,
  start: number,
  end: number,
  selected: string,
  groups: GroupMeta[],
): { index: number; end: number; groupId: string } | null {
  const window = text.slice(start, end);
  let best: { index: number; end: number; groupId: string } | null = null;
  for (const group of groups || []) {
    const groupId = String(group.group_id || "").trim();
    if (!groupId || groupId === selected) continue;
    for (const candidate of getGroupRouteCandidates(group)) {
      for (const idx of getRouteTokenOccurrences(window, candidate)) {
        const absIdx = start + idx;
        if (!best || absIdx >= best.index) {
          best = { index: absIdx, end: absIdx + 1 + candidate.length, groupId };
        }
      }
    }
  }
  return best;
}

export interface ComposerMentionContext {
  scope: "selected" | "destination";
  mentionTargetGroupId: string;
}

// Decide the actor scope for an `@` being typed at ``atIndex``: a valid,
// nearest, same-segment `#group` before it switches the mention to that target
// group's actors; otherwise the local group.
export function resolveComposerMentionContext({
  text,
  atIndex,
  selectedGroupId,
  groups,
}: {
  text: string;
  atIndex: number;
  selectedGroupId: string;
  groups: GroupMeta[];
}): ComposerMentionContext {
  const src = String(text || "");
  const selected = String(selectedGroupId || "").trim();
  const segStart = _segmentStart(src, atIndex);
  const best = _latestValidHashInRange(src, segStart, atIndex, selected, groups);
  if (best) {
    return { scope: "destination", mentionTargetGroupId: best.groupId };
  }
  return { scope: "selected", mentionTargetGroupId: "" };
}

// Extract an explicit target-group agent: the first `@token` that appears after
// a valid `#group` token within the same segment. Returns "" when there is no
// such in-segment `@` (a bare `@`, or a `@` on a different line, is never a
// target agent).
export function extractSegmentTargetActor({
  text,
  selectedGroupId,
  groups,
}: {
  text: string;
  selectedGroupId: string;
  groups: GroupMeta[];
}): string {
  const src = String(text || "");
  const selected = String(selectedGroupId || "").trim();
  const best = _latestValidHashInRange(src, 0, src.length, selected, groups);
  if (!best) return "";
  const segEndNl = src.indexOf("\n", best.end);
  const segEnd = segEndNl >= 0 ? segEndNl : src.length;
  const segment = src.slice(best.end, segEnd);
  const match = segment.match(/(?:^|\s)@([^\s,，。@#]+)/);
  return match ? match[1].trim() : "";
}

export function buildComposerMentionSuggestions({
  kind,
  filter,
  recipientActors,
  groups,
}: {
  kind: ComposerMentionKind;
  filter: string;
  recipientActors: Actor[];
  groups: GroupMeta[];
}): ComposerMentionSuggestion[] {
  const needle = String(filter || "").trim().toLowerCase();
  return kind === "agent"
    ? buildAgentMentionSuggestions(recipientActors, needle)
    : buildGroupMentionSuggestions(groups, needle);
}
