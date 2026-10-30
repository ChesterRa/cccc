import type { Task } from "../../types";

/** The part of a task a board search matched, when the card does not show it. */
export interface TaskSearchExcerpt {
  field: "title" | "outcome" | "notes" | "handoff_to";
  before: string;
  match: string;
  after: string;
}

// Text before the match is kept short, measured in display width, so the
// match stays within the quote's two lines even after wide emoji or CJK.
const BEFORE_WIDTH = 20;
const AFTER = 60;
// Context is cut from a window around the match, so long notes stay cheap.
const WINDOW = 400;
const CHUNK = 4096;

/**
 * The original [start, end) of the character whose lowercase holds `unit` of
 * the lowercased text. Whole chunks are cased natively to skip ahead; only the
 * chunk holding the unit is walked character by character. A character's
 * lowercase length does not depend on its neighbours, so the chunks add up.
 */
function originalCharacter(text: string, unit: number): [number, number] {
  let lowered = 0;
  let index = 0;
  while (index < text.length) {
    let next = Math.min(text.length, index + CHUNK);
    // Keep a surrogate pair in one chunk.
    const last = text.charCodeAt(next - 1);
    if (next < text.length && last >= 0xd800 && last <= 0xdbff) next += 1;
    const length = text.slice(index, next).toLowerCase().length;
    if (lowered + length > unit) break;
    lowered += length;
    index = next;
  }
  while (index < text.length) {
    const char = String.fromCodePoint(text.codePointAt(index) ?? 0);
    const length = char.toLowerCase().length;
    if (lowered + length > unit) return [index, index + char.length];
    lowered += length;
    index += char.length;
  }
  return [text.length, text.length];
}

/** Original [start, end) of the first case-insensitive match, or null. */
function findMatch(text: string, query: string): [number, number] | null {
  const lowered = text.toLowerCase();
  const at = lowered.indexOf(query);
  if (at < 0) return null;
  // No character changed length, so offsets carry over unchanged.
  if (lowered.length === text.length) return [at, at + query.length];
  // Casing lengthened a character (İ becomes two units): map both ends back.
  return [originalCharacter(text, at)[0], originalCharacter(text, at + query.length - 1)[1]];
}

// Intl.Segmenter is not in the ES2020 lib this project compiles against.
type GraphemeSegmenter = new (
  locales: undefined,
  options: { granularity: "grapheme" },
) => { segment(text: string): Iterable<{ segment: string }> };

/** User-perceived characters; a browser without Intl.Segmenter splits by code point. */
function graphemes(text: string): string[] {
  const Segmenter = (Intl as { Segmenter?: GraphemeSegmenter }).Segmenter;
  if (!Segmenter) return Array.from(text);
  const segmenter = new Segmenter(undefined, { granularity: "grapheme" });
  return Array.from(segmenter.segment(text), (part) => part.segment);
}

const flatten = (text: string) => text.replace(/\s+/g, " ");

const WIDE =
  /[\p{Extended_Pictographic}\u1100-\u115f\u2e80-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe30-\ufe4f\uff00-\uff60\uffe0-\uffe6\u{20000}-\u{3fffd}]/u;
const NARROW_WORD = /^[\p{L}\p{N}]$/u;
const narrowWord = (grapheme: string | undefined) =>
  !!grapheme && NARROW_WORD.test(grapheme) && !WIDE.test(grapheme);

/** The end of `head` that fits the width budget, starting at a whole word where words are spaced. */
function lead(head: string[]): string[] {
  let width = 0;
  let from = head.length;
  while (from > 0) {
    const next = width + (WIDE.test(head[from - 1]) ? 2 : 1);
    if (next > BEFORE_WIDTH) break;
    width = next;
    from -= 1;
  }
  const kept = head.slice(from);
  // A cut inside a spaced word drops that word's remaining letters.
  if (from > 0 && narrowWord(head[from - 1]) && narrowWord(kept[0])) {
    const space = kept.findIndex((grapheme) => /\s/.test(grapheme));
    if (space >= 0) return kept.slice(space + 1);
  }
  return kept;
}

function excerpt(text: string, [start, end]: [number, number]) {
  const head = graphemes(text.slice(Math.max(0, start - WINDOW), start));
  const tail = graphemes(text.slice(end, end + WINDOW));
  const before = lead(head).join("");
  const after = tail.slice(0, AFTER).join("");
  return {
    before: (start - before.length > 0 ? "…" : "") + flatten(before).trimStart(),
    match: flatten(text.slice(start, end)),
    after: flatten(after).trimEnd() + (end + after.length < text.length ? "…" : ""),
  };
}

/**
 * Where the search matched a task, unless the text passed as shown (the
 * openings of what the card displays) already contains it; a quote may still
 * repeat a match the card shows further on. The server matches these fields;
 * a match it found but this cannot (casing that differs in context) shows
 * nothing rather than a wrong place.
 */
export function taskSearchExcerpt(
  task: Task,
  query: string,
  shown: ReadonlyArray<string | null | undefined>,
): TaskSearchExcerpt | null {
  // Cased like the server's query: the whole string, so context rules agree.
  const wanted = query.trim().toLowerCase();
  if (!wanted) return null;
  if (shown.some((text) => text && findMatch(text, wanted))) return null;
  for (const field of ["title", "outcome", "notes", "handoff_to"] as const) {
    const text = String(task[field] || "");
    const found = text && findMatch(text, wanted);
    if (found) return { field, ...excerpt(text, found) };
  }
  return null;
}
