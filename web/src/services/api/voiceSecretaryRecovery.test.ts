import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { retryVoiceAssistantTranscriptPersistence } from "./voiceSecretary";

const pending = [
  { segment_id: "external-v-600", text: "second", start_ms: 600, end_ms: 1100 },
  { segment_id: "external-v-1200", text: "third", start_ms: 1200, end_ms: 1800 },
];
const payload = {
  sessionId: "original-session",
  documentPath: "docs/meeting.md",
  text: "first\nsecond\nthird",
  language: "zh-CN",
  modelId: "volcengine:test",
  recognitionBackend: "external_provider_asr_final",
  pendingSegments: pending,
};
const success = () =>
  new Response(
    JSON.stringify({
      ok: true,
      result: { group_id: "original-group", session_id: payload.sessionId },
    }),
  );
const failure = () =>
  new Response(
    JSON.stringify({ ok: false, error: { code: "io_error", message: "injected failure" } }),
    { status: 500 },
  );

describe("Voice Secretary checkpoint recovery", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it.each([true, false])(
    "recovers original segments before finalization (partial=%s)",
    async (partial) => {
      vi.stubGlobal("window", { location: { search: "" } });
      const fetch = vi.spyOn(globalThis, "fetch").mockImplementation(async () => success());
      const result = await retryVoiceAssistantTranscriptPersistence("original-group", {
        ...payload,
        partial,
      });
      expect(result.ok).toBe(true);
      const requests = fetch.mock.calls.map(([url, init]) => {
        expect(String(url)).toContain("/groups/original-group/");
        return JSON.parse(String(init?.body));
      });
      expect(requests.map((r) => r.segment_id)).toEqual(
        partial
          ? ["external-v-600", "external-v-1200"]
          : ["external-v-600", "external-v-1200", "final-asr"],
      );
      for (const [i, segment] of pending.entries()) {
        expect(requests[i]).toMatchObject({
          ...segment,
          session_id: payload.sessionId,
          document_path: payload.documentPath,
          language: payload.language,
          transcript_stage: "live",
          is_final: true,
          flush: true,
          source_model_id: payload.modelId,
          trigger: { recognition_backend: "external_provider_asr_streaming" },
        });
        expect(requests[i].revision_only).not.toBe(true);
        expect(requests[i].supersede_stage).not.toBe("live");
      }
      if (!partial)
        expect(requests[2]).toMatchObject({ revision_only: true, supersede_stage: "live" });
    },
  );

  it.each([false, true])(
    "retains the remaining IDs and text after HTTP/network failure (network=%s)",
    async (network) => {
      vi.stubGlobal("window", { location: { search: "" } });
      const fetch = vi
        .spyOn(globalThis, "fetch")
        .mockImplementationOnce(async () => success())
        .mockImplementationOnce(async () => {
          if (network) throw new Error("offline");
          return failure();
        })
        .mockImplementation(async () => success());
      const result = await retryVoiceAssistantTranscriptPersistence("original-group", payload);
      expect(result.ok).toBe(false);
      if (result.ok) throw new Error("expected failure");
      expect(fetch).toHaveBeenCalledTimes(2);
      expect(result.error.details).toMatchObject({ transcript_pending_segments: [pending[1]] });
      const remaining = (result.error.details as { transcript_pending_segments: unknown })
        .transcript_pending_segments;
      const retried = await retryVoiceAssistantTranscriptPersistence("original-group", {
        ...payload,
        pendingSegments: remaining,
      });
      expect(retried.ok).toBe(true);
      expect(fetch.mock.calls.map(([, init]) => JSON.parse(String(init?.body)).segment_id)).toEqual(
        ["external-v-600", "external-v-1200", "external-v-1200", "final-asr"],
      );
    },
  );

  it("waits for checkpoint acknowledgement before issuing the next write", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    let acknowledge!: (response: Response) => void;
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            acknowledge = resolve;
          }),
      )
      .mockImplementation(async () => success());
    const recovering = retryVoiceAssistantTranscriptPersistence("original-group", payload);
    expect(fetch).toHaveBeenCalledTimes(1);
    await Promise.resolve();
    expect(fetch).toHaveBeenCalledTimes(1);
    acknowledge(success());
    expect((await recovering).ok).toBe(true);
    expect(fetch).toHaveBeenCalledTimes(3);
  });

  it("retains unconfirmed input when reading the response body fails", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetch = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        new ReadableStream({
          start(controller) {
            controller.error(new Error("connection reset after headers"));
          },
        }),
      ),
    );
    const result = await retryVoiceAssistantTranscriptPersistence("original-group", payload);
    expect(result.ok).toBe(false);
    if (result.ok) throw new Error("expected body read failure");
    expect(result.error.details).toMatchObject({ transcript_pending_segments: pending });
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("rejects malformed pending data before writing a superseding revision", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetch = vi.spyOn(globalThis, "fetch");
    const result = await retryVoiceAssistantTranscriptPersistence("original-group", {
      ...payload,
      pendingSegments: [pending[0], { text: "missing identity" }],
    });
    expect(result.ok).toBe(false);
    expect(fetch).not.toHaveBeenCalled();
  });
});
