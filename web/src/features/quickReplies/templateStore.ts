import { useEffect } from "react";
import { create } from "zustand";
import { onAuthChange } from "../../services/api/base";
import {
  fetchQuickReplyTemplates,
  saveQuickReplyTemplates,
  type QuickReplyDraftEntry,
  type QuickReplyTemplateList,
} from "../../services/api/quickReplyTemplates";
import { replyEntriesOf, type QuickReplyEntry } from "./effective";

const DEFAULT_LIMIT = 24;

type TemplateState = {
  /** The person's replies in display order. Until `loaded`, nobody knows which default replies
   *  are changed or deleted, so no list offers one. */
  replies: QuickReplyEntry[];
  revision: number;
  limit: number;
  loaded: boolean;
  /** Why the first read failed, until a read succeeds. Once a list is shown, a failed reread
   *  keeps it and leaves this empty. */
  loadError: string | null;
  /** Changes whenever this browser's credentials change; the list then starts empty again. */
  epoch: number;
};

export const useQuickReplyTemplateStore = create<TemplateState>(() => ({
  replies: [],
  revision: 0,
  limit: DEFAULT_LIMIT,
  loaded: false,
  loadError: null,
  epoch: 0,
}));

export type QuickReplyTemplatesResult = { ok: true } | { ok: false; code: string; message: string };

let loading: Promise<QuickReplyTemplatesResult> | null = null;
let readOnce = false;
// Requests are numbered as they start. A read that started before a save was shown may
// describe the list from before that save, so its answer is dropped; a later read is shown
// as it is, whatever its revision.
let sequence = 0;
let savedAt = 0;

// The list belongs to whoever is signed in. When that changes it is cleared at once, and an
// answer to a request made before the change is never shown.
function forget() {
  loading = null;
  readOnce = false;
  useQuickReplyTemplateStore.setState((state) => ({
    replies: [],
    revision: 0,
    limit: DEFAULT_LIMIT,
    loaded: false,
    loadError: null,
    epoch: state.epoch + 1,
  }));
}

onAuthChange(forget);

const currentEpoch = () => useQuickReplyTemplateStore.getState().epoch;

function show(list: QuickReplyTemplateList) {
  useQuickReplyTemplateStore.setState({
    replies: Array.isArray(list.replies) ? replyEntriesOf(list.replies) : [],
    revision: list.revision,
    limit: list.limit || DEFAULT_LIMIT,
    loaded: true,
    loadError: null,
  });
}

function failed(error: { code: string; message?: string }): QuickReplyTemplatesResult {
  return { ok: false, code: error.code, message: error.message || error.code };
}

/** Read the list again; overlapping calls share one request, and a failure keeps what is shown. */
export function loadQuickReplyTemplates(): Promise<QuickReplyTemplatesResult> {
  readOnce = true;
  if (loading) return loading;
  const epoch = currentEpoch();
  const started = ++sequence;
  const request = fetchQuickReplyTemplates()
    .then((response): QuickReplyTemplatesResult => {
      if (!response.ok) {
        const state = useQuickReplyTemplateStore.getState();
        if (epoch === currentEpoch() && !state.loaded) {
          useQuickReplyTemplateStore.setState({
            loadError: response.error.message || response.error.code,
          });
        }
        return failed(response.error);
      }
      if (epoch === currentEpoch() && started > savedAt) show(response.result);
      return { ok: true };
    })
    .finally(() => {
      if (loading === request) loading = null;
    });
  loading = request;
  return request;
}

/** The chat page reads the person's templates once, and again after a change of sign-in, so
 *  each message's quick reply menu has them before it opens; the rows only subscribe. The
 *  settings tab and the composer's template list read again when opened. A read-only page
 *  offers no quick replies and reads nothing. */
export function usePreloadQuickReplyTemplates(enabled: boolean): void {
  const epoch = useQuickReplyTemplateStore((state) => state.epoch);
  useEffect(() => {
    if (enabled && !readOnce) void loadQuickReplyTemplates();
  }, [enabled, epoch]);
}

/** Save the whole list over the revision the edit started from. A save made elsewhere in
 *  between answers quick_reply_templates_conflict and changes nothing. */
export async function saveQuickReplyTemplateList(
  baseRevision: number,
  drafts: QuickReplyDraftEntry[],
): Promise<QuickReplyTemplatesResult> {
  const epoch = currentEpoch();
  const response = await saveQuickReplyTemplates(baseRevision, drafts);
  if (!response.ok) return failed(response.error);
  if (epoch === currentEpoch()) {
    savedAt = ++sequence;
    show(response.result);
  }
  return { ok: true };
}

/** Tests only: forget the list and whether it was read, as a change of sign-in does. */
export function resetQuickReplyTemplatesForTest() {
  forget();
}
