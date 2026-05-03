import { describe, expect, it } from "vitest";

import {
  shouldAutoOpenVoiceReplyBubble,
  voiceTranscriptItemsFromMeetingSession,
} from "../../../src/pages/chat/voice-secretary/voiceComposerUtils";

describe("voice composer utils", () => {
  it("opens a reply bubble for local requests with a final reply", () => {
    expect(shouldAutoOpenVoiceReplyBubble({
      requestId: "request-1",
      replyText: "reply",
      dismissKey: "request-1\u0001done\u0001reply",
      isLocalRequest: true,
    })).toBe(true);
  });

  it("opens a reply bubble when an existing active request receives a new final reply", () => {
    expect(shouldAutoOpenVoiceReplyBubble({
      requestId: "request-1",
      replyText: "reply",
      dismissKey: "request-1\u0001done\u0001reply",
      previousReplyKey: "active:working:2026-05-03T07:22:01Z",
    })).toBe(true);
  });

  it("does not reopen a dismissed reply bubble", () => {
    expect(shouldAutoOpenVoiceReplyBubble({
      requestId: "request-1",
      replyText: "reply",
      dismissKey: "request-1\u0001done\u0001reply",
      isLocalRequest: true,
      wasDismissed: true,
    })).toBe(false);
  });

  it("does not open old restored final replies without an observed active state", () => {
    expect(shouldAutoOpenVoiceReplyBubble({
      requestId: "request-1",
      replyText: "reply",
      dismissKey: "request-1\u0001done\u0001reply",
    })).toBe(false);
  });

  it("does not restore ask or prompt sessions into document transcript", () => {
    expect(voiceTranscriptItemsFromMeetingSession({
      session_id: "voice-ask",
      capture_mode: "instruction",
      document_path: "",
      diarization: {
        speaker_transcript_segments: [
          { speaker_label: "Speaker 1", text: "ask text", start_ms: 0, end_ms: 1000 },
        ],
      },
    }, { documentPathFallback: "docs/voice.md" })).toEqual([]);

    expect(voiceTranscriptItemsFromMeetingSession({
      session_id: "voice-prompt",
      capture_mode: "prompt",
      document_path: "docs/voice.md",
      diarization: {
        speaker_transcript_segments: [
          { speaker_label: "Speaker 1", text: "prompt text", start_ms: 0, end_ms: 1000 },
        ],
      },
    })).toEqual([]);
  });
});
