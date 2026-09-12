import { describe, expect, it } from "vite-plus/test";

import { voiceTranscriptItemsFromMeetingSession } from "./voiceComposerUtils";
import { mergeVoiceTranscriptItems, replaceVoiceTranscriptSessionItems } from "./voiceStreamModel";
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

  it("does not let an unstable revision hide the stable live transcript", () => {
    const live = {
      session_id: "session-1",
      segment_id: "live-1",
      transcript_stage: "live",
      is_final: true,
      text: "stable live text",
    };
    const partialFinal = {
      session_id: "session-1",
      segment_id: "partial-final",
      transcript_stage: "final",
      supersede_stage: "live",
      supersedes_segment_ids: ["live-1"],
      is_final: false,
      text: "partial final text",
    };

    expect(projectVoiceTranscriptRevisions([live, partialFinal])).toEqual([live]);
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
      id: JSON.stringify(["session-1", "final-asr"]),
      text: "最终文本。",
      source: "assistant_service_local_asr_final",
      sourceLabel: "Final SenseVoice",
    });
    expect(items[1]).toMatchObject({
      id: JSON.stringify(["session-2", "other-session-live"]),
      text: "另一场会议的实时文本",
      sourceLabel: "Live Paraformer",
    });
  });
  it("keeps both recordings across final revision, document reload, and repeated text", () => {
    const documentPath = "docs/voice-secretary/meeting.md";
    const recording = (id: string, text: string) => ({
      session_id: id,
      capture_mode: "document",
      document_path: documentPath,
      segments: [
        {
          session_id: id,
          segment_id: "final-asr",
          transcript_stage: "final",
          supersede_stage: "live",
          text,
          created_at: "2026-09-10T09:50:33Z",
        },
      ],
    });
    const first = recording("recording-1", "第一次录音");
    const second = recording("recording-2", "第二次录音");
    const firstItems = voiceTranscriptItemsFromMeetingSession(first);
    const secondItems = voiceTranscriptItemsFromMeetingSession(second);
    const saved = replaceVoiceTranscriptSessionItems(firstItems, secondItems);
    expect(saved.map((item) => item.text).sort()).toEqual(["第一次录音", "第二次录音"].sort());
    const restored = voiceTranscriptItemsFromMeetingSession({
      ...first,
      session_id: "document-aggregate",
      segments: [...first.segments, ...second.segments],
    });
    expect(restored.map((item) => item.sessionId)).toEqual(["recording-1", "recording-2"]);
    expect(new Set(restored.map((item) => item.id)).size).toBe(2);
    expect(mergeVoiceTranscriptItems(saved, restored)).toHaveLength(2);
    const repeatedText = voiceTranscriptItemsFromMeetingSession({
      ...first,
      session_id: "document-aggregate",
      segments: [...first.segments, ...recording("recording-2", "第一次录音").segments],
    });
    expect(mergeVoiceTranscriptItems([], repeatedText)).toHaveLength(2);
  });
});
