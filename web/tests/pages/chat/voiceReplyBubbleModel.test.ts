import { describe, expect, it } from "vitest";

import {
  resolveAutoOpenVoiceReplyBubbleRequestId,
  trackActiveVoiceReplyRequests,
  type VoiceReplyBubbleTracker,
} from "../../../src/pages/chat/voice-secretary/voiceReplyBubbleModel";

function createTracker(): VoiceReplyBubbleTracker {
  return {
    replyKeyByRequestId: new Map(),
    localRequestIds: new Set(),
    dismissedReplyKeys: new Set(),
  };
}

describe("voice reply bubble model", () => {
  it("opens the reply bubble when an observed active request becomes a final reply", () => {
    const tracker = createTracker();

    trackActiveVoiceReplyRequests(tracker, [{
      request_id: "request-1",
      status: "working",
      request_text: "同安还未下雨吗",
      created_at: "2026-05-03T07:20:00Z",
      updated_at: "2026-05-03T07:20:01Z",
    }]);

    const requestId = resolveAutoOpenVoiceReplyBubbleRequestId(tracker, {
      request_id: "request-1",
      status: "done",
      request_text: "同安还未下雨吗",
      reply_text: "同安现在显示小雨，接下来两小时仍有雨。",
      created_at: "2026-05-03T07:20:00Z",
      updated_at: "2026-05-03T07:22:01Z",
    });

    expect(requestId).toBe("request-1");
  });

  it("does not open restored old final replies without an observed active state", () => {
    const tracker = createTracker();

    const requestId = resolveAutoOpenVoiceReplyBubbleRequestId(tracker, {
      request_id: "request-1",
      status: "done",
      request_text: "同安还未下雨吗",
      reply_text: "同安现在显示小雨，接下来两小时仍有雨。",
      created_at: "2026-05-03T07:20:00Z",
      updated_at: "2026-05-03T07:22:01Z",
    });

    expect(requestId).toBe("");
  });

  it("does not reopen the same dismissed final reply", () => {
    const tracker = createTracker();
    const dismissKey = "request-1\u0001done\u0001同安现在显示小雨，接下来两小时仍有雨。";
    tracker.localRequestIds.add("request-1");
    tracker.dismissedReplyKeys.add(dismissKey);

    const requestId = resolveAutoOpenVoiceReplyBubbleRequestId(tracker, {
      request_id: "request-1",
      status: "done",
      reply_text: "同安现在显示小雨，接下来两小时仍有雨。",
    });

    expect(requestId).toBe("");
  });
});
