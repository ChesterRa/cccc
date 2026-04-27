import { describe, expect, it } from "vitest";

import { resolveVoiceServiceReadiness } from "../../../src/pages/chat/voice-secretary/voiceServiceReadiness";

describe("voiceServiceReadiness", () => {
  it("recognizes installed streaming runtime and backend from fresh assistant state", () => {
    const readiness = resolveVoiceServiceReadiness({
      streamingRuntimeId: "sherpa_onnx_streaming",
      assistant: {
        assistant_id: "voice_secretary",
        kind: "voice_secretary",
        enabled: true,
        lifecycle: "idle",
        health: {
          service: {
            streaming_backend: { ready: true },
          },
        },
        config: {
          recognition_backend: "assistant_service_local_asr",
        },
      },
      serviceRuntimesById: {
        sherpa_onnx_streaming: {
          runtime_id: "sherpa_onnx_streaming",
          status: "ready",
        },
      },
    });

    expect(readiness).toMatchObject({
      assistantEnabled: true,
      serviceAsrReady: true,
      streamingRuntimeReady: true,
      serviceAsrConfigured: true,
    });
  });
});
