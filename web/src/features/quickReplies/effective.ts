import type { QuickReplyListEntry } from "../../services/api/quickReplyTemplates";

/** Stable identities for the four defaults; the saved name and text are personal content. */
export const QUICK_REPLY_KINDS = ["proceed", "why", "rethink", "choices"] as const;
export type QuickReplyKind = (typeof QUICK_REPLY_KINDS)[number];
export type QuickReplySource = QuickReplyKind | `custom:${string}`;

/** One reply in the person's list: a default reply, whose fields left out follow the interface
 *  language, or one the person wrote. */
export type QuickReplyEntry =
  | { builtin: QuickReplyKind; name?: string; text?: string }
  | { id: string; name: string; text: string };

const isKind = (value: unknown): value is QuickReplyKind =>
  (QUICK_REPLY_KINDS as readonly unknown[]).includes(value);

/** The entries this page can show; the daemon accepts no other kinds of default reply. */
export function replyEntriesOf(entries: readonly QuickReplyListEntry[]): QuickReplyEntry[] {
  return entries.filter(
    (entry): entry is QuickReplyEntry => !("builtin" in entry) || isKind(entry.builtin),
  );
}

export const QUICK_REPLY_EMOJI: Record<QuickReplyKind, string> = {
  proceed: "👍",
  why: "🤔",
  rethink: "🔄",
  choices: "☑️",
};

type Translate = (key: string) => string;
export type EffectiveReplyTemplate = {
  source: QuickReplySource;
  name: string;
  text: string;
  emoji?: string;
};

/** Defaults stay in the interface language; a stored field overrides only that field. */
export function builtinReplyDefault(t: Translate, kind: QuickReplyKind) {
  return { name: t(`quickReply.${kind}`), text: t(`quickReply.${kind}Text`) };
}

/** Both sending and insertion use this list, in the person's saved order, only after their
 *  settings were read. A default reply's unchanged name or text follows the interface language. */
export function effectiveReplyTemplates(
  t: Translate,
  replies: readonly QuickReplyEntry[],
): EffectiveReplyTemplate[] {
  return replies.map((reply) => {
    if ("builtin" in reply) {
      const defaults = builtinReplyDefault(t, reply.builtin);
      return {
        source: reply.builtin,
        name: reply.name ?? defaults.name,
        text: reply.text ?? defaults.text,
        // An edited reply may mean something else, so its original action emoji no longer fits.
        ...(reply.name === undefined && reply.text === undefined
          ? { emoji: QUICK_REPLY_EMOJI[reply.builtin] }
          : {}),
      };
    }
    return { source: `custom:${reply.id}` as const, name: reply.name, text: reply.text };
  });
}

/** The first line with text, shown under the name so people see what they insert. */
export function templatePreview(text: string): string {
  return (
    text
      .split("\n")
      .map((line) => line.trim())
      .find(Boolean) ?? ""
  );
}
