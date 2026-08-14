import { describe, expect, it } from "vite-plus/test";

import {
  createVoiceRecordingSessionScope,
  voiceRecordingCaptureMode,
  voiceRecordingDispatchTarget,
  voiceRecordingTargetDocumentPath,
  voiceRecordingTargetGroupId,
} from "./voiceRecordingSessionScope";

const scope = createVoiceRecordingSessionScope({
  runId: 7,
  sessionId: " session-1 ",
  groupId: " old-group ",
  documentPath: " docs/meeting.md ",
  captureMode: "document",
  dispatchTarget: "document",
  composerText: "original prompt",
  composerContext: { source: "old-group" },
});

describe("voice recording session scope", () => {
  it("keeps recording writes bound to the group and document selected at start", () => {
    expect(voiceRecordingTargetGroupId(scope, "new-group")).toBe("old-group");
    expect(voiceRecordingTargetDocumentPath(scope, "docs/new.md")).toBe("docs/meeting.md");
  });

  it("keeps capture routing stable when the visible group changes assistant state", () => {
    expect(voiceRecordingCaptureMode(scope, "prompt")).toBe("document");
    expect(voiceRecordingDispatchTarget(scope, "composer")).toBe("document");
  });

  it("copies and freezes the composer context snapshot", () => {
    const mutableContext = { source: "group-a" };
    const frozen = createVoiceRecordingSessionScope({ ...scope, composerContext: mutableContext });
    mutableContext.source = "group-b";

    expect(frozen.composerContext).toEqual({ source: "group-a" });
    expect(Object.isFrozen(frozen)).toBe(true);
    expect(Object.isFrozen(frozen.composerContext)).toBe(true);
  });
});
