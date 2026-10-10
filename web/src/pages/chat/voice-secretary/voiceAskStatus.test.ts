import { describe, expect, it } from "vite-plus/test";
import type { AssistantVoiceAskFeedback, SecretaryTaskSummary } from "../../../types";
import { normalizeAssistantStateResult } from "../../../services/api/groups";
import { currentVoiceAskTask, voiceAskReplyIsCurrent } from "./voiceAskStatus";
import { resolveAutoOpenVoiceReplyBubbleRequestId } from "./voiceReplyBubbleModel";
import { voiceReplyDismissKey } from "./voiceComposerUtils";

const previous: SecretaryTaskSummary = {
  task_id: "previous-task",
  target: {
    group_id: "A",
    scope_key: "scope",
    kind: "ask",
    document_path: "",
    request_id: "request",
  },
  phase: "needs_user",
  cleanup_confirmed: true,
  created_at: "",
  updated_at: "",
  source_count: 1,
  preview: "Question",
  diagnostic: "",
  projection_error: "",
  previous_task_id: "",
  superseded_by: "",
  candidate_available: false,
  projected_at: "confirmed",
};
const successor: SecretaryTaskSummary = {
  ...previous,
  task_id: "successor-task",
  phase: "queued",
  previous_task_id: "previous-task",
  projected_at: "",
};

describe("Ask task and reply authority", () => {
  it("does not replace an accepted successor with its earlier task's delayed snapshot", () => {
    expect(currentVoiceAskTask([previous], "A", "request", successor)).toEqual(successor);
    expect(
      currentVoiceAskTask(
        [{ ...previous, target: { ...previous.target, group_id: "B" } }],
        "A",
        "request",
        successor,
      ),
    ).toEqual(successor);
  });

  it("matches a final canonical result to its task after that task leaves the recent window", () => {
    const cached = { ...successor, phase: "done" as const };
    const feedback: AssistantVoiceAskFeedback = {
      request_id: "request",
      status: "done",
      reply_text: "Answer",
      secretary_task_id: "successor-task",
    };
    expect(voiceAskReplyIsCurrent(cached, feedback)).toBe(true);
    expect(
      voiceAskReplyIsCurrent(cached, { ...feedback, secretary_task_id: "previous-task" }),
    ).toBe(false);
    expect(voiceAskReplyIsCurrent(successor, feedback)).toBe(true);
    expect(voiceAskReplyIsCurrent(successor, { ...feedback, secretary_task_id: "" })).toBe(false);
  });

  it("does not let an old result ID settle a newer task even if its projection marker exists", () => {
    expect(
      voiceAskReplyIsCurrent(
        { ...successor, phase: "done", projected_at: "confirmed" },
        {
          request_id: "request",
          status: "done",
          reply_text: "Old answer",
          secretary_task_id: "previous-task",
        },
      ),
    ).toBe(false);
  });

  it("preserves canonical task identity while normalizing status responses", () => {
    const result = normalizeAssistantStateResult("A", {
      ask_requests: [
        {
          request_id: "request",
          status: "done",
          reply_text: "Answer",
          secretary_task_id: "successor-task",
        },
      ],
    });
    expect(result.ask_requests?.[0]).toMatchObject({ secretary_task_id: "successor-task" });
  });

  it("opens each result once and recognizes a retry with identical reply text as a new result", () => {
    const tracker = {
      replyKeyByRequestId: new Map<string, string>(),
      localRequestIds: new Set(["request"]),
      dismissedReplyKeys: new Set<string>(),
    };
    const result = {
      request_id: "request",
      status: "done",
      reply_text: "Answer",
      secretary_task_id: "previous-task",
    };
    expect(resolveAutoOpenVoiceReplyBubbleRequestId(tracker, result)).toBe("request");
    expect(resolveAutoOpenVoiceReplyBubbleRequestId(tracker, result)).toBe("");
    tracker.dismissedReplyKeys.add(voiceReplyDismissKey(result));
    expect(
      resolveAutoOpenVoiceReplyBubbleRequestId(tracker, {
        ...result,
        secretary_task_id: "successor-task",
      }),
    ).toBe("request");
  });
});
