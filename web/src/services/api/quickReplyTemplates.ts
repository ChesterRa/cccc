import { apiJson } from "./base";

/** One reply in the signed-in person's list, in display order: a default reply (`proceed`,
 *  `why`, `rethink`, `choices`), whose `name` or `text` left out shows the default in the
 *  interface language, or a reply the person wrote. */
export type QuickReplyListEntry =
  | { builtin: string; name?: string; text?: string }
  | { id: string; name: string; text: string };

export type QuickReplyTemplateList = {
  /** Display order; a default reply the person deleted is not listed. */
  replies: QuickReplyListEntry[];
  /** 0 before the first save. */
  revision: number;
  updated_at: string;
  limit: number;
};

/** A reply being saved; a new one of the person's own has no id yet and the daemon assigns it. */
export type QuickReplyDraftEntry =
  | { builtin: string; name?: string; text?: string }
  | { id?: string; name: string; text: string };

const PATH = "/api/v1/quick-reply-templates";

export function fetchQuickReplyTemplates() {
  return apiJson<QuickReplyTemplateList>(PATH, { cache: "no-store" });
}

/** Replaces the whole list. `ifRevision` is the revision last read, so a save made in another
 *  browser in between is reported as `quick_reply_templates_conflict` instead of overwritten. */
export function saveQuickReplyTemplates(ifRevision: number, replies: QuickReplyDraftEntry[]) {
  return apiJson<QuickReplyTemplateList>(PATH, {
    method: "PUT",
    body: JSON.stringify({ if_revision: ifRevision, replies }),
  });
}
