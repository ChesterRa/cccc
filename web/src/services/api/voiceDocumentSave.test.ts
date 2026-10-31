import { afterEach, describe, expect, it, vi } from "vite-plus/test";
import { saveVoiceAssistantDocument } from "./groups";

const sha = "a".repeat(64);
const target = "b".repeat(64);
describe("Voice Secretary conditional document save", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });
  it("sends the original content version and resolved target together", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi
      .spyOn(globalThis, "fetch")
      .mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: true,
            result: {
              document: {
                document_id: "d",
                document_path: "voice/a.md",
                content: "",
                content_sha256: sha,
                content_target_id: target,
              },
            },
          }),
        ),
      );
    const response = await saveVoiceAssistantDocument("g", {
      documentPath: "voice/a.md",
      content: "",
      expectedContentSha256: sha,
      expectedTargetId: target,
    });
    expect(JSON.parse(String(fetchMock.mock.calls[0]?.[1]?.body))).toMatchObject({
      document_path: "voice/a.md",
      content: "",
      expected_content_sha256: sha,
      expected_target_id: target,
    });
    expect(response.ok && response.result.document?.content).toBe("");
    expect(response.ok && response.result.document?.content_target_id).toBe(target);
  });
});
