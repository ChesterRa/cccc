import { useCallback } from "react";
import { useComposerStore } from "../stores/useComposerStore";
import type { ComposerMentionKind } from "../pages/chat/chatMentionSuggestions";
import { resolveControlledComposerMentionContext } from "./composerGroupMentions";

export function useRemoteComposerMention(
  kind: ComposerMentionKind,
  scope: "selected" | "destination",
  triggerIndex: number | null | undefined,
) {
  return useComposerStore(
    useCallback(
      (state) =>
        kind === "agent" && scope === "destination"
          ? resolveControlledComposerMentionContext({
              text: state.composerText,
              atIndex: triggerIndex ?? state.composerText.lastIndexOf("@"),
              tokens: state.composerGroupMentionTokens,
            }).remote
          : undefined,
      [kind, scope, triggerIndex],
    ),
  );
}
