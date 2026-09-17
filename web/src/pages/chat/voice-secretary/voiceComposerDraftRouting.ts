import { useComposerStore } from "../../../stores/useComposerStore";
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
}): "active" | "draft" | "ignored" {
  const groupId = String(input.groupId || "").trim();
  const text = String(input.text || "").trim();
  if (!groupId || !text) return "ignored";

  const state = useComposerStore.getState();
  if (String(state.activeGroupId || "").trim() === groupId) {
    state.setComposerText((current) => mergeVoiceComposerDraftText(current, text, input.mode));
    return "active";
  }

  state.upsertDraft(groupId, (draft) => {
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
  return "draft";
}
