import { describe, expect, it } from "vite-plus/test";

import { voicePromptRequestOwnership } from "./voiceComposerUtils";

describe("voicePromptRequestOwnership", () => {
  it("allows reuse only inside the owning Group", () => {
    const request = { requestId: "request-a", pendingGroupId: "group-a" };

    expect(voicePromptRequestOwnership({ ...request, targetGroupId: "group-a" })).toBe(
      "same_group",
    );
    expect(voicePromptRequestOwnership({ ...request, targetGroupId: "group-b" })).toBe(
      "other_group",
    );
  });

  it("does not infer completion from elapsed time", () => {
    expect(
      voicePromptRequestOwnership({
        requestId: "request-a",
        pendingGroupId: "group-a",
        targetGroupId: "group-b",
      }),
    ).toBe("other_group");
  });

  it("releases ownership when the pending request has been cleared", () => {
    expect(
      voicePromptRequestOwnership({
        requestId: "",
        pendingGroupId: "group-a",
        targetGroupId: "group-b",
      }),
    ).toBe("none");
  });

  const finishedRequest = {
    requestId: "request-a",
    pendingGroupId: "group-a",
    targetGroupId: "group-b",
    task: {
      target: { request_id: "request-a", group_id: "group-a" },
      phase: "failed",
      cleanup_confirmed: true,
    },
  };

  it("allows an explicit new request after the owning task failed or was cancelled and cleaned up", () => {
    expect(voicePromptRequestOwnership(finishedRequest)).toBe("none");
    expect(
      voicePromptRequestOwnership({
        ...finishedRequest,
        task: { ...finishedRequest.task, phase: "cancelled" },
      }),
    ).toBe("none");
  });

  it("keeps running, unresolved and not-yet-cleaned work owned regardless of its age", () => {
    for (const phase of ["queued", "running", "needs_user", "unconfirmed", "done"]) {
      expect(
        voicePromptRequestOwnership({
          ...finishedRequest,
          task: { ...finishedRequest.task, phase },
        }),
      ).toBe("other_group");
    }
    expect(
      voicePromptRequestOwnership({
        ...finishedRequest,
        task: { ...finishedRequest.task, cleanup_confirmed: false },
      }),
    ).toBe("other_group");
  });

  it("does not let a stale terminal result release another request's ownership", () => {
    expect(voicePromptRequestOwnership({ ...finishedRequest, requestId: "replacement" })).toBe(
      "other_group",
    );
    expect(
      voicePromptRequestOwnership({ ...finishedRequest, pendingGroupId: "another-group" }),
    ).toBe("other_group");
  });
});
