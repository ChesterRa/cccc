import { describe, expect, it } from "vite-plus/test";

import {
  createVoiceTranscriptPreview,
  voiceTranscriptPreviewBelongsToGroup,
} from "./voiceStreamModel";

function createPreview(groupId?: string) {
  return createVoiceTranscriptPreview({
    id: "voice-preview-1",
    cleanText: "live transcript",
    phase: "interim",
    pendingFinalText: "",
    metadata: { mode: "prompt", groupId },
    now: 100,
  });
}

describe("voice transcript preview group visibility", () => {
  it("shows a live transcript only in its recording group", () => {
    const preview = createPreview("recording-group");

    expect(voiceTranscriptPreviewBelongsToGroup(preview, "recording-group")).toBe(true);
    expect(voiceTranscriptPreviewBelongsToGroup(preview, "new-group")).toBe(false);
  });

  it("keeps legacy previews without a group id visible", () => {
    expect(voiceTranscriptPreviewBelongsToGroup(createPreview(), "selected-group")).toBe(true);
  });
});
