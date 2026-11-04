import {
  isComposerMentionBoundary,
  isComposerMentionStartBoundary,
} from "../../hooks/composerGroupMentions";
import { isEmailAt } from "../../utils/mentionEmail";
import type { ComposerMentionKind } from "./chatMentionSuggestions";

export type ComposerInputSelection = { text: string; start: number; end: number };

export type ComposerTextEdit = { start: number; oldEnd: number; newEnd: number };

export function findComposerMentionAtCaret(text: string, caret: number, selectionEnd = caret) {
  const end = Math.max(0, Math.min(text.length, caret));
  if (end !== selectionEnd) return null;
  const prefix = text.slice(0, end);
  const at = prefix.lastIndexOf("@");
  const hash = prefix.lastIndexOf("#");
  const start = Math.max(at, hash);
  if (start < 0) return null;
  if (!isComposerMentionStartBoundary(prefix[start - 1] || "")) return null;
  if (isEmailAt(text, start)) return null;
  let segmentStart = start;
  while (segmentStart > 0 && !/\s/u.test(prefix[segmentStart - 1])) segmentStart -= 1;
  if (/(?:https?:\/\/|www\.)/iu.test(prefix.slice(segmentStart, start))) return null;
  const filter = prefix.slice(start + 1);
  if (/\s/u.test(filter)) return null;
  return { kind: (at > hash ? "agent" : "group") as ComposerMentionKind, start, end, filter };
}

export function getComposerTextEdit(
  previous: string,
  next: string,
  selection: ComposerInputSelection | null,
  caret: number,
): ComposerTextEdit | null {
  if (!selection || selection.text !== previous || previous === next) return null;
  const newEnd = Math.max(0, Math.min(next.length, caret));
  const start = Math.min(selection.start, newEnd);
  const oldEnd = previous.length - (next.length - newEnd);
  if (
    start < 0 ||
    oldEnd < start ||
    newEnd < start ||
    previous.slice(0, start) !== next.slice(0, start) ||
    previous.slice(oldEnd) !== next.slice(newEnd)
  )
    return null;
  return { start, oldEnd, newEnd };
}

export function rebaseComposerMentionTokens<T extends { start: number; end: number }>(
  tokens: T[],
  edit: ComposerTextEdit | null,
): T[] {
  if (!edit) return tokens;
  const delta = edit.newEnd - edit.oldEnd;
  let changed = false;
  const result: T[] = [];
  for (const token of tokens) {
    if (token.end <= edit.start) {
      result.push(token);
    } else if (token.start >= edit.oldEnd) {
      changed ||= delta !== 0;
      result.push(delta ? { ...token, start: token.start + delta, end: token.end + delta } : token);
    } else {
      changed = true;
    }
  }
  return changed ? result : tokens;
}

export function retainComposerMentionSuffix<
  T extends { start: number; end: number; token: string },
>(text: string, tokens: T[], live: T[], triggerEnd: number | undefined): T[] {
  if (triggerEnd === undefined || tokens === live) return live;
  const valid = new Set(live);
  return tokens.filter(
    (token) =>
      valid.has(token) ||
      (token.start >= triggerEnd && text.slice(token.start, token.end) === token.token),
  );
}

export function replaceComposerMention(
  text: string,
  trigger: { start: number; end: number },
  token: string,
) {
  const before = text.slice(0, trigger.start);
  const after = text.slice(trigger.end);
  const leading = before && !isComposerMentionStartBoundary(before[before.length - 1]) ? " " : "";
  const trailing = !after || !isComposerMentionBoundary(after[0]) ? " " : "";
  const inserted = `${leading}${token}${trailing}`;
  const start = before.length + leading.length;
  return {
    text: before + inserted + after,
    tokenStart: start,
    caret: before.length + inserted.length + (!trailing && after.startsWith(" ") ? 1 : 0),
    edit: { start: trigger.start, oldEnd: trigger.end, newEnd: trigger.start + inserted.length },
  };
}
