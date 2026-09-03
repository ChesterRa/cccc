import { describe, expect, it } from "vite-plus/test";

import { voiceTranscriptItemsFromMeetingSession } from "./voiceComposerUtils";
import { projectVoiceTranscriptRevisions } from "./voiceTranscriptRevisions";

describe("projectVoiceTranscriptRevisions", () => {
  it("shows the final revision while preserving unrelated live segments", () => {
    const segments = [
      { segment_id: "live-1", transcript_stage: "live", text: "raw one" },
      { segment_id: "live-2", transcript_stage: "live", text: "raw two" },
      { segment_id: "other-live", transcript_stage: "live", text: "other" },
      {
        segment_id: "final-asr",
        transcript_stage: "final",
        text: "final text",
        supersedes_segment_ids: ["live-1", "live-2"],
      },
    ];

    expect(projectVoiceTranscriptRevisions(segments)).toEqual([segments[2], segments[3]]);
    expect(segments).toHaveLength(4);
  });

  it("keeps legacy segments that have no revision metadata", () => {
    const segments = [{ segment_id: "legacy", text: "legacy text" }];
    expect(projectVoiceTranscriptRevisions(segments)).toEqual(segments);
  });

  it("restores the final SenseVoice revision instead of superseded Live Paraformer cards", () => {
    const items = voiceTranscriptItemsFromMeetingSession({
      session_id: "session-1",
      capture_mode: "document",
      document_path: "docs/voice-secretary/meeting.md",
      segments: [
        {
          session_id: "session-1",
          segment_id: "live-1",
          text: "没有标点的实时文本",
          transcript_stage: "live",
          trigger: { recognition_backend: "assistant_service_local_asr_streaming" },
        },
        {
          session_id: "session-1",
          segment_id: "final-asr",
          text: "最终文本。",
          transcript_stage: "final",
          supersede_stage: "live",
          supersedes_segment_ids: ["live-1"],
          trigger: {
            recognition_backend: "assistant_service_local_asr_final",
            final_model_id: "sense-voice",
          },
        },
        {
          session_id: "session-1",
          segment_id: "late-live",
          text: "迟到的实时文本",
          transcript_stage: "live",
          trigger: { recognition_backend: "assistant_service_local_asr_streaming" },
        },
        {
          session_id: "session-2",
          segment_id: "other-session-live",
          text: "另一场会议的实时文本",
          transcript_stage: "live",
          trigger: { recognition_backend: "assistant_service_local_asr_streaming" },
        },
      ],
    });

    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({
      id: "final-asr",
      text: "最终文本。",
      source: "assistant_service_local_asr_final",
      sourceLabel: "Final SenseVoice",
    });
    expect(items[1]).toMatchObject({
      id: "other-session-live",
      text: "另一场会议的实时文本",
      sourceLabel: "Live Paraformer",
    });
  });
});
