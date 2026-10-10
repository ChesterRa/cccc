import { describe, expect, it } from "vite-plus/test";

import {
  resolveAutoOpenVoiceReplyBubbleRequestId,
  trackActiveVoiceReplyRequests,
  type VoiceReplyBubbleTracker,
} from "../../../src/pages/chat/voice-secretary/voiceReplyBubbleModel";
import { voiceReplyDismissKey } from "../../../src/pages/chat/voice-secretary/voiceComposerUtils";

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

    trackActiveVoiceReplyRequests(tracker, [
      {
        request_id: "request-1",
        status: "working",
        request_text: "同安还未下雨吗",
        created_at: "2026-05-03T07:20:00Z",
        updated_at: "2026-05-03T07:20:01Z",
      },
    ]);

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
    const reply = {
      request_id: "request-1",
      status: "done",
      reply_text: "同安现在显示小雨，接下来两小时仍有雨。",
    };
    tracker.localRequestIds.add("request-1");
    tracker.dismissedReplyKeys.add(voiceReplyDismissKey(reply));

    const requestId = resolveAutoOpenVoiceReplyBubbleRequestId(tracker, reply);

    expect(requestId).toBe("");
  });

  it("opens a new task's reply even when its text matches the dismissed predecessor", () => {
    const tracker = createTracker();
    const reply = {
      request_id: "request-1",
      secretary_task_id: "task-1",
      status: "done",
      reply_text: "Answer",
    };
    tracker.localRequestIds.add(reply.request_id);
    expect(resolveAutoOpenVoiceReplyBubbleRequestId(tracker, reply)).toBe(reply.request_id);
    tracker.dismissedReplyKeys.add(voiceReplyDismissKey(reply));
    expect(
      resolveAutoOpenVoiceReplyBubbleRequestId(tracker, { ...reply, secretary_task_id: "task-2" }),
    ).toBe(reply.request_id);
  });

  it("does not reopen a local request's result after that same result has already been seen", () => {
    const tracker = createTracker();
    const reply = {
      request_id: "request-1",
      secretary_task_id: "task-1",
      status: "done",
      reply_text: "Answer",
    };
    tracker.localRequestIds.add(reply.request_id);
    expect(resolveAutoOpenVoiceReplyBubbleRequestId(tracker, reply)).toBe(reply.request_id);
    expect(resolveAutoOpenVoiceReplyBubbleRequestId(tracker, { ...reply })).toBe("");
  });
});
