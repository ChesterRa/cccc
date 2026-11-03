import type { Actor, ChatMessageData } from "../types";

export type MessageMentionRange = { start: number; end: number; kind: "agent" | "group" };

export function getMessageMentionTokens(actors: Actor[], data: unknown): string[] {
  const names = actors.flatMap((actor) => [actor.id, actor.title || ""]).map((name) => name.trim());
  const refs = Array.isArray((data as ChatMessageData | undefined)?.refs)
    ? (data as ChatMessageData).refs || []
    : [];
  return [
    ...new Set([
      ...names.filter(Boolean).map((name) => (name.startsWith("@") ? name : `@${name}`)),
      ...refs.flatMap((ref) =>
        ref &&
        (ref.kind === "local_group_route" || ref.kind === "connect_group_ref") &&
        typeof ref.token === "string"
          ? [ref.token]
          : [],
      ),
    ]),
  ];
}

export function messageMentionColor(kind: "agent" | "group"): string {
  return kind === "group" ? "rgba(56, 189, 248, 0.34)" : "rgba(167, 139, 250, 0.34)";
}

export function findMessageMentionRanges(
  text: string,
  knownTokens: string[] = [],
): MessageMentionRange[] {
  const candidates = [...new Set(knownTokens.filter((token) => /^[@#]\S/u.test(token)))].sort(
    (a, b) => b.length - a.length,
  );
  const ranges: MessageMentionRange[] = [];
  const protectedRanges = [
    ...text.matchAll(
      /(?:https?:\/\/|www\.)[^\s<>"']+|[\p{L}\p{N}._%+-]+@[\p{L}\p{N}.-]+\.[\p{L}]{2,}/giu,
    ),
  ].map((match) => ({ start: match.index, end: match.index + match[0].length }));
  let protectedIndex = 0;
  const markers = /[@#]/gu;
  for (const match of text.matchAll(markers)) {
    const start = match.index;
    if (ranges.length && start < ranges[ranges.length - 1].end) continue;
    while (protectedIndex < protectedRanges.length && protectedRanges[protectedIndex].end <= start)
      protectedIndex += 1;
    if (protectedRanges[protectedIndex] && protectedRanges[protectedIndex].start <= start) continue;
    const before = text[start - 1] || "";
    if (before && /[A-Za-z0-9_./+\\#-]/u.test(before)) continue;
    const segmentStart = Math.max(
      text.lastIndexOf(" ", start - 1),
      text.lastIndexOf("\n", start - 1),
      text.lastIndexOf("\t", start - 1),
    );
    if (/(?:https?:\/\/|www\.)/iu.test(text.slice(segmentStart + 1, start))) continue;

    const known = candidates.find((token) => {
      if (!text.startsWith(token, start)) return false;
      const after = text[start + token.length] || "";
      return !after || /[\s,，。.!?;；:：)）\]】]/u.test(after);
    });
    const fallback = text.slice(start).match(/^[@#][\p{L}\p{N}_][\p{L}\p{N}\p{M}_.-]*/u)?.[0];
    const token = known || (fallback || "").replace(/[.-]+$/u, "");
    if (token.length < 2 || ["@", "/"].includes(text[start + token.length] || "")) continue;
    ranges.push({ start, end: start + token.length, kind: token[0] === "@" ? "agent" : "group" });
  }
  return ranges;
}
