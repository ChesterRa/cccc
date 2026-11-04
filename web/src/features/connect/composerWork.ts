import type { ComposerState } from "../../stores/useComposerStore";

/** Work held by the embedded composer that would be lost if its page closed. */
export function hasUnsentComposerWork(state: ComposerState): boolean {
  if (Object.values(state.failedSendsByGroup).some((items) => items.length > 0)) return true;
  if (state.composerText.trim() || state.composerFiles.length > 0) return true;
  // The active Group's saved entry may predate edits or clearing its composer.
  return Object.entries(state.drafts).some(
    ([groupId, draft]) =>
      groupId !== state.activeGroupId &&
      (draft.composerText.trim().length > 0 || draft.composerFiles.length > 0),
  );
}
