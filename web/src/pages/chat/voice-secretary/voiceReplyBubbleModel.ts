import type { AssistantVoiceAskFeedback } from "../../../types";
import {
  askFeedbackStatusKey,
  hasFinalAskReply,
  shouldAutoOpenVoiceReplyBubble,
  voiceReplyDismissKey,
} from "./voiceComposerUtils";

export type VoiceReplyBubbleTracker = {
  replyKeyByRequestId: Map<string, string>;
  localRequestIds: Set<string>;
  dismissedReplyKeys: Set<string>;
};

export function trackActiveVoiceReplyRequests(
  tracker: VoiceReplyBubbleTracker,
  items: AssistantVoiceAskFeedback[],
): void {
  for (const item of items) {
    const requestId = String(item.request_id || "").trim();
    if (!requestId || hasFinalAskReply(item)) continue;
    tracker.replyKeyByRequestId.set(
      requestId,
      `active:${askFeedbackStatusKey(item.status)}:${String(item.updated_at || item.created_at || "")}`,
    );
  }
}

export function resolveAutoOpenVoiceReplyBubbleRequestId(
  tracker: VoiceReplyBubbleTracker,
  item: AssistantVoiceAskFeedback | null | undefined,
): string {
  if (!item || !hasFinalAskReply(item)) return "";
  const requestId = String(item.request_id || "").trim();
  const replyText = String(item.reply_text || "").trim();
  const dismissKey = voiceReplyDismissKey(item);
  const previousReplyKey = tracker.replyKeyByRequestId.get(requestId) || "";
  tracker.replyKeyByRequestId.set(requestId, dismissKey);
  const isLocalRequest = tracker.localRequestIds.has(requestId);
  if (!shouldAutoOpenVoiceReplyBubble({
    requestId,
    replyText,
    dismissKey,
    previousReplyKey,
    isLocalRequest,
    wasDismissed: tracker.dismissedReplyKeys.has(dismissKey),
  })) {
    return "";
  }
  return requestId;
}
