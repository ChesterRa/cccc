import { getActorAccentIndex, type Actor, type ChatMessageData } from "../types";
import { CHET_SELECTOR } from "./chetIdentity";
import { isEmailAt } from "./mentionEmail";

export type MessageMentionKind = "agent" | "group";
/** A reference a message can mark. An agent's carries the accent slot its name has in message headers. */
export type MessageMentionToken = { text: string; accent?: number };
export type MessageMentionRange = {
  start: number;
  end: number;
  kind: MessageMentionKind;
  accent?: number;
};

export type MessageLinkRange = { start: number; end: number; href: string };

/** The selectors the composer suggests next to this Group's agents. */
const SELECTOR_MENTIONS = ["@all", "@foreman", "@peers", CHET_SELECTOR];

// Links may contain @ or # after punctuation.
const LINK = /(?:https?:\/\/|www\.)[^\s<>"'`，。！？；：“”‘’【】《》（）、]+/giu;

/** Bare web links in plain messages. Trailing prose punctuation stays outside the link. */
export function findMessageLinkRanges(text: string): MessageLinkRange[] {
  const ranges: MessageLinkRange[] = [];
  for (const match of text.matchAll(LINK)) {
    const start = match.index;
    if (start > 0 && /[A-Za-z0-9_@]/u.test(text[start - 1])) continue;
    let value = match[0].replace(/[.,!?;:]+$/u, "");
    for (const [open, close] of [
      ["(", ")"],
      ["[", "]"],
    ]) {
      let openings = 0;
      let closings = 0;
      for (const character of value) {
        if (character === open) openings += 1;
        else if (character === close) closings += 1;
      }
      let end = value.length;
      while (end > 0 && value[end - 1] === close && closings > openings) {
        end -= 1;
        closings -= 1;
      }
      value = value.slice(0, end);
    }
    const href = /^www\./iu.test(value) ? `http://${value}` : value;
    try {
      const parsed = new URL(href);
      if (!value || !["http:", "https:"].includes(parsed.protocol) || !parsed.hostname) continue;
    } catch {
      continue;
    }
    ranges.push({ start, end: start + value.length, href });
  }
  return ranges;
}

/**
 * The references a message can show as highlighted: this Group's current agents by id or title,
 * the composer's selectors, and the Group references the message carries. A removed or renamed
 * agent is no longer recognized in older messages.
 */
export function getMessageMentionTokens(actors: Actor[], data: unknown): MessageMentionToken[] {
  const tokens = new Map<string, MessageMentionToken>();
  const add = (text: string, accent?: number) => {
    if (!tokens.has(text)) tokens.set(text, accent === undefined ? { text } : { text, accent });
  };
  const mention = (name: string) => (name.startsWith("@") ? name : `@${name}`);
  for (const selector of SELECTOR_MENTIONS) add(selector);
  // An exact id wins over another agent's title, as in recipient resolution.
  for (const actor of actors) {
    const id = actor.id.trim();
    if (id) add(mention(id), getActorAccentIndex(id));
  }
  // A title several agents share names no one of them, so it stays neutral. Titles are
  // compared as the reference they show, ignoring ASCII case, as recipient resolution does.
  const fold = (token: string) => token.replace(/[A-Z]/gu, (c) => c.toLowerCase());
  const titled = actors.flatMap((actor) => {
    const title = (actor.title || "").trim();
    return title ? [{ id: actor.id, token: mention(title) }] : [];
  });
  const owners = new Map<string, Set<string>>();
  for (const { id, token } of titled) {
    owners.set(fold(token), (owners.get(fold(token)) ?? new Set<string>()).add(id));
  }
  for (const { id, token } of titled) {
    add(token, owners.get(fold(token))?.size === 1 ? getActorAccentIndex(id) : undefined);
  }
  const refs = (data as ChatMessageData | undefined)?.refs;
  if (Array.isArray(refs)) {
    for (const ref of refs) {
      if (
        ref &&
        (ref.kind === "local_group_route" || ref.kind === "connect_group_ref") &&
        typeof ref.token === "string"
      )
        add(ref.token);
    }
  }
  return [...tokens.values()];
}

// A name that ends in an ASCII letter or digit ends where those characters stop, so
// "@claude-1你看" still matches; a name in another script needs a space or punctuation after it.
function endsAtBoundary(token: string, next: string | undefined): boolean {
  if (!next) return true;
  return /[A-Za-z0-9_]$/u.test(token)
    ? !/[A-Za-z0-9_@/-]/u.test(next)
    : !/[\p{L}\p{N}\p{M}_@/-]/u.test(next);
}

/** Finds the known references in a message text; any other @ or # word is left as text. */
export function findMessageMentionRanges(
  text: string,
  knownTokens: readonly MessageMentionToken[],
): MessageMentionRange[] {
  if (!/[@#]/u.test(text)) return [];
  const candidates = knownTokens
    .filter((token) => /^[@#]\S/u.test(token.text))
    .sort((a, b) => b.text.length - a.text.length);
  if (!candidates.length) return [];
  const links = findMessageLinkRanges(text);
  const ranges: MessageMentionRange[] = [];
  let link = 0;
  for (const match of text.matchAll(/[@#]/gu)) {
    const start = match.index;
    if (ranges.length && start < ranges[ranges.length - 1].end) continue;
    while (link < links.length && links[link].end <= start) link += 1;
    if (link < links.length && links[link].start <= start) continue;
    if (start > 0 && /[A-Za-z0-9_.%+/\\#@-]/u.test(text[start - 1])) continue;
    if (isEmailAt(text, start)) continue;
    const token = candidates.find(
      (candidate) =>
        text.startsWith(candidate.text, start) &&
        endsAtBoundary(candidate.text, text[start + candidate.text.length]),
    );
    if (!token) continue;
    const range: MessageMentionRange = {
      start,
      end: start + token.text.length,
      kind: token.text[0] === "@" ? "agent" : "group",
    };
    if (token.accent !== undefined) range.accent = token.accent;
    ranges.push(range);
  }
  return ranges;
}
