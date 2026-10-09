import { useComposerStore } from "../../../stores/useComposerStore";
import { hashComposerSnapshot } from "./voiceComposerUtils";
import {
  pruneComposerAgentMentionTokens,
  pruneComposerGroupMentionTokens,
} from "../../../hooks/composerGroupMentions";

export type VoiceComposerDraftMode = "replace" | "append";

export function mergeVoiceComposerDraftText(
  current: string,
  transcript: string,
  mode: VoiceComposerDraftMode,
): string {
  const text = String(transcript || "").trim();
  if (!text) return String(current || "");
  const existing = String(current || "");
  if (mode === "replace" || !existing.trim()) return text;
  return `${existing.replace(/\s+$/g, "")}\n\n${text}`;
}

export function routeVoiceTextToComposerGroup(input: {
  groupId: string;
  text: string;
  mode: VoiceComposerDraftMode;
  /** Omit only for direct dictation or an explicit user decision to apply a reviewed draft. */
  expectedSnapshotHash?: string;
}): "active" | "draft" | "changed" | "ignored" {
  const groupId = String(input.groupId || "").trim();
  const text = String(input.text || "").trim();
  if (!groupId || !text) return "ignored";

  const state = useComposerStore.getState();
  const matches = (text: string) =>
    input.expectedSnapshotHash === undefined ||
    hashComposerSnapshot(text) === input.expectedSnapshotHash;
  if (String(state.activeGroupId || "").trim() === groupId) {
    let changed = false;
    state.setComposerText((current) => {
      // Check inside the same synchronous store update that applies the result.
      if (!matches(current)) {
        changed = true;
        return current;
      }
      return mergeVoiceComposerDraftText(current, text, input.mode);
    });
    return changed ? "changed" : "active";
  }

  let changed = false;
  state.upsertDraft(groupId, (draft) => {
    if (!matches(draft?.composerText || "")) {
      changed = true;
      return draft;
    }
    const composerText = mergeVoiceComposerDraftText(draft?.composerText || "", text, input.mode);
    return {
      composerText,
      composerGroupMentionTokens: pruneComposerGroupMentionTokens({
        text: composerText,
        tokens: draft?.composerGroupMentionTokens || [],
      }),
      composerAgentMentionTokens: pruneComposerAgentMentionTokens({
        text: composerText,
        tokens: draft?.composerAgentMentionTokens || [],
      }),
      composerFiles: draft?.composerFiles || [],
      toText: draft?.toText || "",
      replyTarget: draft?.replyTarget || null,
      quotedPresentationRef: draft?.quotedPresentationRef || null,
      quotedVoiceDocumentRef: draft?.quotedVoiceDocumentRef || null,
      messageMode: draft?.messageMode || state.preferredMessageMode,
    };
  });
  return changed ? "changed" : "draft";
}
