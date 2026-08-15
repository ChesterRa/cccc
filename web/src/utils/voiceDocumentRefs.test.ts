import { describe, expect, it } from "vite-plus/test";
import type { AssistantVoiceDocument } from "../types";
import {
  buildVoiceDocumentMessageRef,
  getVoiceDocumentMessageRefs,
  getVoiceDocumentRefLabel,
  voiceDocumentRefMatchesDocument,
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

  it("matches an archived document by stable id or its pre-archive path", () => {
    const ref = buildVoiceDocumentMessageRef("group-1", document);
    expect(ref).not.toBeNull();
    expect(
      voiceDocumentRefMatchesDocument(ref!, "group-1", {
        ...document,
        document_path: "archive/meeting-notes.md",
      }),
    ).toBe(true);
    expect(
      voiceDocumentRefMatchesDocument({ ...ref!, document_id: undefined }, "group-1", document),
    ).toBe(true);
  });

  it("does not match another group or document", () => {
    const ref = buildVoiceDocumentMessageRef("group-1", document);
    expect(ref).not.toBeNull();
    expect(voiceDocumentRefMatchesDocument(ref!, "group-2", document)).toBe(false);
    expect(
      voiceDocumentRefMatchesDocument(ref!, "group-1", {
        ...document,
        document_id: "doc-2",
        document_path: "voice/other.md",
      }),
    ).toBe(false);
  });
});
