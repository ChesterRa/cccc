import { describe, expect, it } from "vitest";

import {
  DEFAULT_LIVE_SERVICE_MODEL_ID,
  resolveLocalAsrModels,
  supportsOfflineAsrModel,
  supportsStreamingAsrModel,
} from "../../../../src/components/modals/settings/assistantsLocalAsrModels";

const finalModel = {
  model_id: "sherpa_onnx_sense_voice_zh_en_ja_ko_yue_int8",
  kind: "asr",
  title: "sherpa-onnx SenseVoice zh/en/ja/ko/yue int8",
  status: "downloading",
  progress_percent: 19,
  total_size_bytes: 163002883,
  offline: { engine: "sense_voice" },
  streaming: {},
};

const liveModel = {
  model_id: "sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en",
  kind: "asr",
  title: "sherpa-onnx streaming Paraformer trilingual zh/cantonese/en int8",
  status: "not_installed",
  total_size_bytes: 1047671211,
  offline: {},
  streaming: { engine: "paraformer" },
};

describe("assistants local ASR model selection", () => {
  it("treats empty capability objects as unsupported", () => {
    expect(supportsOfflineAsrModel(finalModel)).toBe(true);
    expect(supportsStreamingAsrModel(finalModel)).toBe(false);
    expect(supportsOfflineAsrModel(liveModel)).toBe(false);
    expect(supportsStreamingAsrModel(liveModel)).toBe(true);
  });

  it("keeps final and streaming model cards separate while final model downloads", () => {
    const selected = resolveLocalAsrModels({
      configuredModelId: finalModel.model_id,
      serviceModels: [finalModel, liveModel],
      serviceModelsById: {
        [finalModel.model_id]: finalModel,
        [liveModel.model_id]: liveModel,
      },
    });

    expect(selected.finalModel?.model_id).toBe(finalModel.model_id);
    expect(selected.finalModel?.status).toBe("downloading");
    expect(selected.liveModel?.model_id).toBe(liveModel.model_id);
    expect(selected.liveModel?.status).toBe("not_installed");
  });

  it("falls back to the default streaming model when state omits streaming ASR models", () => {
    const selected = resolveLocalAsrModels({
      configuredModelId: finalModel.model_id,
      serviceModels: [finalModel],
      serviceModelsById: {
        [finalModel.model_id]: finalModel,
      },
    });

    expect(selected.finalModel?.model_id).toBe(finalModel.model_id);
    expect(selected.liveModel?.model_id).toBe(DEFAULT_LIVE_SERVICE_MODEL_ID);
    expect(selected.liveModel?.status).toBe("not_installed");
    expect(selected.liveModel?.streaming).toEqual({ engine: "paraformer" });
  });

  it("preserves default streaming model download progress when capability metadata is missing", () => {
    const downloadingLiveModel = {
      model_id: DEFAULT_LIVE_SERVICE_MODEL_ID,
      kind: "asr",
      title: "sherpa-onnx streaming Paraformer trilingual zh/cantonese/en int8",
      status: "downloading",
      progress_percent: 19,
      total_size_bytes: 1047671211,
      streaming: {},
    };
    const selected = resolveLocalAsrModels({
      configuredModelId: finalModel.model_id,
      serviceModels: [finalModel, downloadingLiveModel],
      serviceModelsById: {
        [finalModel.model_id]: finalModel,
        [downloadingLiveModel.model_id]: downloadingLiveModel,
      },
    });

    expect(selected.liveModel?.model_id).toBe(DEFAULT_LIVE_SERVICE_MODEL_ID);
    expect(selected.liveModel?.status).toBe("downloading");
    expect(selected.liveModel?.progress_percent).toBe(19);
    expect(selected.liveModel?.total_size_bytes).toBe(1047671211);
    expect(selected.liveModel?.streaming).toEqual({ engine: "paraformer" });
  });
});
