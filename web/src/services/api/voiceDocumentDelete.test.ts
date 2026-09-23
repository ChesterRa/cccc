import { afterEach, expect, it, vi } from "vite-plus/test";
import { deleteVoiceAssistantDocument } from "./voiceDocumentDelete";
afterEach(() => {
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});
it.each([true, false])(
  "sends the exact delete target and preserves API outcome (%s)",
  async (ok) => {
    vi.stubGlobal("window", { location: { search: "" } });
    const response = ok
      ? { ok: true, result: { group_id: "group/id", document: { status: "deleted" } } }
      : { ok: false, error: { code: "permission_denied", message: "denied" } };
    const fetch = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(new Response(JSON.stringify(response), { status: ok ? 200 : 403 }));
    expect(await deleteVoiceAssistantDocument("group/id", "voice/原文.md")).toMatchObject(response);
    expect(String(fetch.mock.calls[0][0])).toContain(
      "/groups/group%2Fid/assistants/voice_secretary/documents/delete",
    );
    expect(JSON.parse(String(fetch.mock.calls[0][1]?.body))).toEqual({
      document_path: "voice/原文.md",
    });
  },
);
