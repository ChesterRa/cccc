import { describe, expect, it } from "vite-plus/test";

import {
  normalizeVoiceRecognitionLanguageForBackend,
  shouldAutoOpenVoiceReplyBubble,
  stripUncertainSpeakerPrefix,
  visibleVoiceDocuments,
  voiceLanguageOptionValues,
  voiceTranscriptItemsFromMeetingSession,
} from "../../../src/pages/chat/voice-secretary/voiceComposerUtils";

describe("voice composer utils", () => {
  it("keeps archived and deleted documents out of the working list", () => {
    expect(
      visibleVoiceDocuments([
        { document_id: "active", title: "Active", status: "active" },
        { document_id: "legacy", title: "Legacy", status: "" },
        { document_id: "archived", title: "Archived", status: "archived" },
        { document_id: "deleted", title: "Deleted", status: "deleted" },
      ]).map((document) => document.document_id),
    ).toEqual(["active", "legacy"]);
  });

  it("maps mixed language to auto for browser ASR", () => {
    expect(normalizeVoiceRecognitionLanguageForBackend("mixed", "browser_asr")).toBe("auto");
    expect(voiceLanguageOptionValues("mixed", "browser_asr")).not.toContain("mixed");
    expect(voiceLanguageOptionValues("mixed", "assistant_service")).toContain("mixed");
  });

  it("opens a reply bubble for local requests with a final reply", () => {
    expect(
      shouldAutoOpenVoiceReplyBubble({
        requestId: "request-1",
        replyText: "reply",
        dismissKey: "request-1\u0001done\u0001reply",
        isLocalRequest: true,
      }),
    ).toBe(true);
  });

  it("opens a reply bubble when an existing active request receives a new final reply", () => {
    expect(
      shouldAutoOpenVoiceReplyBubble({
        requestId: "request-1",
        replyText: "reply",
        dismissKey: "request-1\u0001done\u0001reply",
        previousReplyKey: "active:working:2026-05-03T07:22:01Z",
      }),
    ).toBe(true);
  });

  it("does not reopen a dismissed reply bubble", () => {
    expect(
      shouldAutoOpenVoiceReplyBubble({
        requestId: "request-1",
        replyText: "reply",
        dismissKey: "request-1\u0001done\u0001reply",
        isLocalRequest: true,
        wasDismissed: true,
      }),
    ).toBe(false);
  });

  it("does not open old restored final replies without an observed active state", () => {
    expect(
      shouldAutoOpenVoiceReplyBubble({
        requestId: "request-1",
        replyText: "reply",
        dismissKey: "request-1\u0001done\u0001reply",
      }),
    ).toBe(false);
  });

  it("does not restore ask or prompt sessions into document transcript", () => {
    expect(
      voiceTranscriptItemsFromMeetingSession(
        {
          session_id: "voice-ask",
          capture_mode: "instruction",
          document_path: "",
          diarization: {
            speaker_transcript_segments: [
              { speaker_label: "Speaker 1", text: "ask text", start_ms: 0, end_ms: 1000 },
            ],
          },
        },
        { documentPathFallback: "docs/voice.md" },
      ),
    ).toEqual([]);

    expect(
      voiceTranscriptItemsFromMeetingSession({
        session_id: "voice-prompt",
        capture_mode: "prompt",
        document_path: "docs/voice.md",
        diarization: {
          speaker_transcript_segments: [
            { speaker_label: "Speaker 1", text: "prompt text", start_ms: 0, end_ms: 1000 },
          ],
        },
      }),
    ).toEqual([]);
  });

  it("filters legacy semantic input sessions that predate capture_mode", () => {
    expect(
      voiceTranscriptItemsFromMeetingSession(
        {
          session_id: "input-legacy-prompt",
          document_path: "docs/voice.md",
          segments: [{ text: "Target: composer", trigger: { trigger_kind: "user_instruction" } }],
        },
        { documentPathFallback: "docs/voice.md" },
      ),
    ).toEqual([]);
  });

  it("keeps legacy document ASR sessions while removing semantic segments", () => {
    const items = voiceTranscriptItemsFromMeetingSession({
      session_id: "meeting-legacy",
      document_path: "docs/voice.md",
      segments: [
        {
          segment_id: "asr",
          text: "会议内容",
          trigger: { recognition_backend: "assistant_service_local_asr_final" },
        },
        { segment_id: "prompt", text: "优化提示词", trigger: { capture_mode: "prompt" } },
      ],
    });

    expect(items).toHaveLength(1);
    expect(items[0]?.text).toBe("会议内容");
  });

  it("does not restore speaker labels produced by the legacy midpoint mapper", () => {
    const items = voiceTranscriptItemsFromMeetingSession({
      session_id: "meeting-midpoint",
      capture_mode: "document",
      document_path: "docs/voice.md",
      segments: [{ segment_id: "raw", text: "多人会议原始整段", start_ms: 0, end_ms: 10_000 }],
      diarization: {
        model_id: "sherpa_onnx_diarization_pyannote_3dspeaker_zh",
        speaker_transcript_model_id: "sherpa_onnx_diarization_pyannote_3dspeaker_zh",
        speaker_transcript_segments: [
          {
            text: "多人会议原始整段",
            start_ms: 0,
            end_ms: 10_000,
            speaker_label: "Speaker 12",
            speaker_index: 11,
          },
        ],
      },
    });

    expect(items).toHaveLength(1);
    expect(items[0]?.speakerLabel).toBeUndefined();
    expect(items[0]?.speakerIndex).toBeUndefined();
  });

  it("strips uncertain speaker placeholders from restored transcript text", () => {
    expect(stripUncertainSpeakerPrefix("Speaker ?: first line\nSpeaker ?: second line")).toBe(
      "first line second line",
    );
  });
});
