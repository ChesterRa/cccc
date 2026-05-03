import { describe, expect, it } from "vitest";

import {
  DEFAULT_LIVE_SERVICE_MODEL_ID,
  DEFAULT_SERVICE_MODEL_ID,
  effectiveServiceModelId,
  resolveServiceModelRuntimeId,
} from "../../../src/pages/chat/voice-secretary/voiceServiceModelRuntime";
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

  it("does not treat offline SenseVoice backend as live caption readiness", () => {
    const readiness = resolveVoiceServiceReadiness({
      streamingRuntimeId: "sherpa_onnx_streaming",
      assistant: {
        assistant_id: "voice_secretary",
        kind: "voice_secretary",
        enabled: true,
        lifecycle: "idle",
        health: {
          service: {
            offline_backend: { ready: true },
          },
        },
        config: {
          recognition_backend: "assistant_service_local_asr",
          service_model_id: "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8",
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
      serviceAsrReady: true,
      streamingRuntimeReady: true,
      serviceAsrConfigured: false,
    });
  });

  it("falls back to the Sherpa streaming runtime without service model metadata", () => {
    expect(
      resolveServiceModelRuntimeId(
        {
          assistant_id: "voice_secretary",
          kind: "voice_secretary",
          enabled: true,
          lifecycle: "idle",
          config: {
            service_model_id: "removed_heavy_asr_model",
          },
        },
        {},
      ),
    ).toBe("sherpa_onnx_streaming");
  });

  it("keeps the live Paraformer model id separate from the final SenseVoice default", () => {
    expect(effectiveServiceModelId("")).toBe(DEFAULT_SERVICE_MODEL_ID);
    expect(effectiveServiceModelId(DEFAULT_LIVE_SERVICE_MODEL_ID)).toBe(DEFAULT_LIVE_SERVICE_MODEL_ID);
  });

  it("uses service model metadata to resolve the streaming runtime", () => {
    expect(
      resolveServiceModelRuntimeId(
        {
          assistant_id: "voice_secretary",
          kind: "voice_secretary",
          enabled: true,
          lifecycle: "idle",
          config: {
            service_model_id: "custom_sherpa_model",
          },
        },
        {
          custom_sherpa_model: {
            model_id: "custom_sherpa_model",
            runtime_id: "sherpa_onnx_streaming",
          },
        },
      ),
    ).toBe("sherpa_onnx_streaming");
  });

});
