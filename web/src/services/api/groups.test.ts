import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  fetchVoiceAssistantStatus,
  fetchVoiceAssistantWorkspace,
  transcribeVoiceAssistantAudio,
  updateVoiceAssistantRecordingLease,
} from "./groups";
import { fetchVoiceAssistantDocumentContent } from "./voiceSecretary";

describe("assistant API helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("requests the compact Voice Secretary status view", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              group_id: "g1",
              assistant: {
                assistant_id: "voice_secretary",
                kind: "voice_secretary",
                enabled: true,
              },
              service_runtime: { runtime_id: "sherpa_onnx_streaming", status: "ready" },
            },
          }),
        ),
      );

    await fetchVoiceAssistantStatus("g1", { promptRequestId: "r1" });

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/assistants/voice_secretary");
    expect(url).toContain("prompt_request_id=r1");
    expect(url).toContain("view=voice_status");
  });

  it("requests the Voice Secretary workspace view", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              group_id: "g1",
              assistant: {
                assistant_id: "voice_secretary",
                kind: "voice_secretary",
                enabled: true,
              },
              documents: [],
              service_runtime: { runtime_id: "sherpa_onnx_streaming", status: "ready" },
            },
          }),
        ),
      );

    await fetchVoiceAssistantWorkspace("g1", { promptRequestId: "r2" });

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/assistants/voice_secretary");
    expect(url).toContain("prompt_request_id=r2");
    expect(url).toContain("view=voice_workspace");
  });

  it("requests one Voice Secretary document with content", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              group_id: "g1",
              documents: [
                {
                  document_id: "d1",
                  document_path: "docs/voice-secretary/a.md",
                  title: "A",
                  status: "active",
                  content: "body",
                },
              ],
            },
          }),
        ),
      );

    const resp = await fetchVoiceAssistantDocumentContent("g1", "docs/voice-secretary/a.md");

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/assistants/voice_secretary/documents");
    expect(url).toContain("document_path=docs%2Fvoice-secretary%2Fa.md");
    expect(url).toContain("include_content=true");
    expect(resp.ok && resp.result.document?.content).toBe("body");
  });

  it("uploads Voice Secretary audio as a binary request body", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(JSON.stringify({ ok: true, result: { group_id: "g1", transcript: "ok" } })),
      );
    const audio = new Blob([new Uint8Array([1, 2, 3, 4])], { type: "audio/pcm" });

    await transcribeVoiceAssistantAudio("g1", { audio, language: "zh-CN", by: "user" });

    const [url, init] = fetchMock.mock.calls[0] || [];
    expect(String(url)).toContain("language=zh-CN");
    expect(String(url)).toContain("by=user");
    expect(init?.body).toBe(audio);
    expect(new Headers(init?.headers).get("content-type")).toBe("audio/pcm");
  });

  it("sends the direct composer target with a recording lease", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: { group_id: "g1", action: "acquire", acquired: true, lease: {} },
          }),
        ),
      );

    await updateVoiceAssistantRecordingLease("g1", {
      action: "acquire",
      ownerId: "owner-1",
      captureMode: "prompt",
      recognitionBackend: "assistant_service_local_asr",
      dispatchTarget: "composer",
    });

    const [, init] = fetchMock.mock.calls[0] || [];
    const body = JSON.parse(String(init?.body || "{}"));
    expect(body.dispatch_target).toBe("composer");
  });
});
