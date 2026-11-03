// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

const mocks = vi.hoisted(() => ({
  fetch: vi.fn(),
  install: vi.fn(),
  remove: vi.fn(),
  t: (key: string, options?: { status?: string }) => options?.status || key,
}));
vi.mock("../../../services/api", () => ({
  fetchGlobalAsrModels: mocks.fetch,
  installGlobalAsrModel: mocks.install,
  removeGlobalAsrModel: mocks.remove,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.t }) }));
import { LocalAsrModels } from "./LocalAsrModels";

describe("global ASR model readiness", () => {
  beforeEach(() => vi.clearAllMocks());

  it.each([
    {
      liveStatus: "not_installed",
      update: false,
      summary: "assistants.localAsrFailed",
      tone: "error",
    },
    {
      liveStatus: "downloading",
      update: false,
      summary: "assistants.localAsrInstalling",
      tone: "neutral",
    },
    {
      liveStatus: "ready",
      update: true,
      summary: "assistants.localAsrUpdateAvailable",
      tone: "neutral",
    },
  ])(
    "uses the visible summary status when a runtime fails",
    async ({ liveStatus, update, summary, tone }) => {
      (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
      const host = document.createElement("div");
      document.body.append(host);
      const root = createRoot(host);
      mocks.fetch.mockResolvedValueOnce({
        ok: true,
        result: {
          service_runtime: { status: "failed" },
          service_models: [],
          service_models_by_id: {
            sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en: {
              model_id: "sherpa_onnx_streaming_paraformer_trilingual_zh_cantonese_en",
              kind: "asr",
              streaming: { engine: "paraformer" },
              status: liveStatus,
              update_available: update,
            },
          },
        },
      });
      try {
        await act(async () => root.render(<LocalAsrModels isActive />));
        const summaryPill = [...host.querySelectorAll("[data-status-tone]")].find(
          (node) => node.textContent === summary,
        );
        expect(summaryPill?.getAttribute("data-status-tone")).toBe(tone);
        // The failed engine is named in words, not by its reported status value.
        expect(
          [...host.querySelectorAll('[data-status-tone="error"]')].some(
            (node) => node.textContent === "assistants.componentStatus.failed",
          ),
        ).toBe(true);
      } finally {
        await act(async () => root.unmount());
        host.remove();
      }
    },
  );

  it("waits for metadata and offers a read-only retry after a failed load", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const host = document.createElement("div");
    document.body.append(host);
    const root = createRoot(host);
    let resolve!: (value: unknown) => void;
    mocks.fetch.mockImplementationOnce(() => new Promise((done) => (resolve = done)));
    try {
      await act(async () => root.render(<LocalAsrModels isActive />));
      expect(host.querySelector('[role="status"]')?.textContent).toBe("common:loading");
      expect(host.textContent).not.toContain("not_installed");
      expect(host.querySelectorAll("button")).toHaveLength(0);
      await act(async () =>
        resolve({ ok: false, error: { message: "model catalog unavailable" } }),
      );
      expect(host.querySelector('[role="alert"]')?.textContent).toBe("model catalog unavailable");
      expect(host.textContent).not.toContain("not_installed");
      mocks.fetch.mockResolvedValueOnce({
        ok: true,
        result: {
          service_runtime: {
            runtime_id: "sherpa_onnx_streaming",
            installed: true,
            status: "ready",
          },
          service_models: [],
          service_models_by_id: {},
        },
      });
      await act(async () => host.querySelector<HTMLButtonElement>("button")!.click());
      const engine = [...host.querySelectorAll("span")].find(
        (node) => node.textContent === "assistants.localAsrEngineLabel",
      );
      expect(engine?.parentElement?.textContent).toContain("ready");
      expect(host.querySelector('[role="alert"]')).toBeNull();
      expect(mocks.fetch).toHaveBeenCalledTimes(2);
      expect(mocks.install).not.toHaveBeenCalled();
      expect(mocks.remove).not.toHaveBeenCalled();
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
