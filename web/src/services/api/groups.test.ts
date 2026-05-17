import { afterEach, describe, expect, it, vi } from "vitest";

import { fetchVoiceAssistantStatus, fetchVoiceAssistantWorkspace } from "./groups";
import { fetchVoiceAssistantDocumentContent } from "./voiceSecretary";

describe("assistant API helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("requests the compact Voice Secretary status view", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        ok: true,
        result: {
          group_id: "g1",
          assistant: { assistant_id: "voice_secretary", kind: "voice_secretary", enabled: true },
          service_runtimes_by_id: {},
        },
      })),
    );

    await fetchVoiceAssistantStatus("g1", { promptRequestId: "r1" });

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/assistants/voice_secretary");
    expect(url).toContain("prompt_request_id=r1");
    expect(url).toContain("view=voice_status");
  });

  it("requests the Voice Secretary workspace view", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        ok: true,
        result: {
          group_id: "g1",
          assistant: { assistant_id: "voice_secretary", kind: "voice_secretary", enabled: true },
          documents: [],
          service_runtimes_by_id: {},
        },
      })),
    );

    await fetchVoiceAssistantWorkspace("g1", { promptRequestId: "r2" });

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/assistants/voice_secretary");
    expect(url).toContain("prompt_request_id=r2");
    expect(url).toContain("view=voice_workspace");
  });

  it("requests one Voice Secretary document with content", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({
        ok: true,
        result: {
          group_id: "g1",
          documents: [{
            document_id: "d1",
            document_path: "docs/voice-secretary/a.md",
            title: "A",
            status: "active",
            content: "body",
          }],
        },
      })),
    );

    const resp = await fetchVoiceAssistantDocumentContent("g1", "docs/voice-secretary/a.md");

    const url = String(fetchMock.mock.calls[0]?.[0] || "");
    expect(url).toContain("/api/v1/groups/g1/assistants/voice_secretary/documents");
    expect(url).toContain("document_path=docs%2Fvoice-secretary%2Fa.md");
    expect(url).toContain("include_content=true");
    expect(resp.ok && resp.result.document?.content).toBe("body");
  });
});
