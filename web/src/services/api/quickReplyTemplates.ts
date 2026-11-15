import { apiJson } from "./base";

/** One of the signed-in person's reply templates: a short name and the text it sends or inserts. */
export type QuickReplyTemplate = { id: string; name: string; text: string };

export type QuickReplyTemplateList = {
  /** Display order. */
  templates: QuickReplyTemplate[];
  /** 0 before the first save. */
  revision: number;
  updated_at: string;
  limit: number;
};

/** A template being saved; a new one has no id yet and the daemon assigns it. */
export type QuickReplyTemplateDraft = { id?: string; name: string; text: string };

const PATH = "/api/v1/quick-reply-templates";

export function fetchQuickReplyTemplates() {
  return apiJson<QuickReplyTemplateList>(PATH, { cache: "no-store" });
}

/** Replaces the whole list. `ifRevision` is the revision last read, so a save made in another
 *  browser in between is reported as `quick_reply_templates_conflict` instead of overwritten. */
export function saveQuickReplyTemplates(ifRevision: number, templates: QuickReplyTemplateDraft[]) {
  return apiJson<QuickReplyTemplateList>(PATH, {
    method: "PUT",
    body: JSON.stringify({ if_revision: ifRevision, templates }),
  });
}
