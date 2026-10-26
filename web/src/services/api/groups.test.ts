import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  attachScope,
  fetchVoiceAssistantStatus,
  fetchVoiceAssistantWorkspace,
  previewGroupCopy,
  transcribeVoiceAssistantAudio,
  updateVoiceAssistantRecordingLease,
  type GroupCopyPreview,
} from "./groups";
import { setAuthToken } from "./base";
import {
  fetchVoiceAssistantDocumentContent,
  retryVoiceAssistantTranscriptPersistence,
} from "./voiceSecretary";

describe("scope attachment API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    setAuthToken("");
  });

  it.each([undefined, true, false])(
    "passes activation %s explicitly only when requested and keeps the added scope receipt",
    async (activate) => {
      vi.stubGlobal("window", { location: { search: "" } });
      setAuthToken("");
      const result = {
        group_id: "group/fixture",
        scope_key: "s_added",
        scope: { scope_key: "s_added", url: "/project", label: "Project", git_remote: "" },
      };
      const fetch = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(new Response(JSON.stringify({ ok: true, result })));

      expect(
        await attachScope(
          "group/fixture",
          "/project/subdirectory",
          activate === undefined ? undefined : { activate },
        ),
      ).toEqual({ ok: true, result });
      expect(fetch).toHaveBeenCalledTimes(1);
      const [url, init] = fetch.mock.calls[0];
      expect(url).toBe("/api/v1/groups/group%2Ffixture/attach");
      expect(init?.method).toBe("POST");
      expect(JSON.parse(String(init?.body))).toEqual({
        path: "/project/subdirectory",
        by: "user",
        ...(activate === undefined ? {} : { activate }),
      });
    },
  );
});

describe("group copy preview API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    setAuthToken("");
  });

  it("preserves native-session flags independently of Codex runtime in an uploaded preview", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    setAuthToken("");
    const preview: GroupCopyPreview = {
      source_group_id: "g_source",
      actors: [
        { id: "native", runtime: "codex", enabled: true, native_session: true },
        { id: "managed", runtime: "codex", enabled: true, native_session: false },
      ],
      requires_reconnect: { native_sessions: true },
    };
    const result = { upload_id: "fixture-upload", preview };
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response(JSON.stringify({ ok: true, result })));
    const file = new File(["fixture zip"], "native-members.zip");

    expect(await previewGroupCopy(file)).toEqual({ ok: true, result });
    expect(fetch).toHaveBeenCalledTimes(1);
    const [url, init] = fetch.mock.calls[0];
    expect(url).toBe("/api/v1/groups/copy/preview_import");
    expect(init?.method).toBe("POST");
    const uploaded = (init!.body as FormData).get("file") as File;
    expect(uploaded.name).toBe(file.name);
    expect(uploaded.size).toBe(file.size);
  });
});

describe("assistant API helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps a no-change Prompt receipt with empty text", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g1",
            prompt_draft: {
              request_id: "r-unchanged",
              status: "no_change",
              draft_text: "",
              composer_snapshot_hash: "original",
            },
          },
        }),
      ),
    );
    const response = await fetchVoiceAssistantStatus("g1", { promptRequestId: "r-unchanged" });
    expect(response.ok).toBe(true);
    if (response.ok)
      expect(response.result.prompt_draft).toMatchObject({
        request_id: "r-unchanged",
        status: "no_change",
        draft_text: "",
        composer_snapshot_hash: "original",
      });
  });

  it("still rejects empty text when the result promises a Prompt candidate", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: true,
          result: {
            group_id: "g1",
            prompt_draft: { request_id: "r-empty", status: "pending", draft_text: "" },
          },
        }),
      ),
    );
    const response = await fetchVoiceAssistantStatus("g1", { promptRequestId: "r-empty" });
    expect(response.ok).toBe(true);
    if (response.ok) expect(response.result.prompt_draft).toBeUndefined();
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

  it.each([undefined, "external_provider_asr_final"])(
    "retries a final revision with backend %s and the idempotent contract",
    async (backend) => {
      vi.stubGlobal("window", { location: { search: "" } });
      const fetchMock = vi
        .spyOn(globalThis, "fetch")
        .mockResolvedValue(
          new Response(
            JSON.stringify({ ok: true, result: { group_id: "g1", session_id: "session-1" } }),
          ),
        );

      const modelId = backend ? "bailian:fun-asr-realtime" : "sense-voice";
      await retryVoiceAssistantTranscriptPersistence("g1", {
        sessionId: "session-1",
        documentPath: "docs/voice-secretary/meeting.md",
        text: "最终文本。",
        language: "zh-CN",
        modelId,
        recognitionBackend: backend,
      });

      const [, init] = fetchMock.mock.calls[0] || [];
      const body = JSON.parse(String(init?.body || "{}"));
      expect(body).toMatchObject({
        segment_id: "final-asr",
        transcript_stage: "final",
        revision_only: true,
        supersede_stage: "live",
        source_model_id: modelId,
        trigger: { recognition_backend: backend || "assistant_service_local_asr_final" },
      });
    },
  );

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
