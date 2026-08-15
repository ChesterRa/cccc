import { describe, expect, it } from "vite-plus/test";
import type { AssistantVoiceDocument } from "../types";
import {
  buildVoiceDocumentMessageRef,
  getVoiceDocumentMessageRefs,
  getVoiceDocumentRefLabel,
} from "./voiceDocumentRefs";

const document: AssistantVoiceDocument = {
  document_id: "doc-1",
  document_path: "voice/meeting-notes.md",
  title: "Meeting notes",
  status: "active",
};

describe("voiceDocumentRefs", () => {
  it("builds a stable group-scoped reference from a working document", () => {
    expect(buildVoiceDocumentMessageRef(" group-1 ", document)).toEqual({
      kind: "voice_document_ref",
      v: 1,
      group_id: "group-1",
      document_path: "voice/meeting-notes.md",
      document_id: "doc-1",
      title: "Meeting notes",
    });
  });

  it("rejects documents without a group or workspace path", () => {
    expect(buildVoiceDocumentMessageRef("", document)).toBeNull();
    expect(
      buildVoiceDocumentMessageRef("group-1", {
        ...document,
        document_path: "",
        workspace_path: "",
      }),
    ).toBeNull();
  });

  it("filters malformed refs and falls back to the file name for labels", () => {
    const refs = getVoiceDocumentMessageRefs([
      { kind: "voice_document_ref", group_id: "group-1", document_path: "voice/raw.md" },
      { kind: "voice_document_ref", group_id: "group-1", document_path: "" },
    ]);
    expect(refs).toHaveLength(1);
    expect(getVoiceDocumentRefLabel(refs[0])).toBe("raw.md");
  });
});
