import { describe, expect, it } from "vite-plus/test";
import { normalizeAssistantVoiceDocument } from "./groups";

describe("Voice Secretary document content", () => {
  it("preserves leading whitespace, trailing whitespace, and explicit empty content", () => {
    for (const content of ["    indented code\n\n", "", "  \n"]) {
      const document = normalizeAssistantVoiceDocument({ document_id: "d", content });
      expect(document?.content).toBe(content);
    }
  });

  it("distinguishes omitted content from an explicitly empty document", () => {
    const document = normalizeAssistantVoiceDocument({ document_id: "d" });
    expect(document?.content).toBeUndefined();
  });
});
