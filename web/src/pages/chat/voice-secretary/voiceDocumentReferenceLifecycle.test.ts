import { describe, expect, it, vi } from "vite-plus/test";
import type { LedgerEvent } from "../../../types";
import { voiceDocumentRefMatchesDocument } from "../../../utils/voiceDocumentRefs";
import {
  archiveVoiceDocumentWithReferenceCleanup,
  archivedVoiceDocumentFromEvent,
  clearArchivedVoiceDocumentReference,
  clearRemovedVoiceDocumentReferences,
  findRemovedVoiceDocuments,
  newVoiceDocumentArchiveEvents,
} from "./voiceDocumentReferenceLifecycle";

describe("voice document reference lifecycle", () => {
  it("detects documents removed by a workspace refresh without treating a path rename as removal", () => {
    const removed = findRemovedVoiceDocuments(
      [
        { document_id: "doc-a", document_path: "voice/a.md", status: "active" },
        { document_id: "doc-b", document_path: "voice/b.md", status: "active" },
      ],
      [{ document_id: "doc-a", document_path: "voice/renamed-a.md", status: "active" }],
    );

    expect(removed.map((document) => document.document_id)).toEqual(["doc-b"]);
  });

  it("projects legacy and canonical archive events back to the pre-archive identity", () => {
    const legacyEvent = {
      kind: "assistant.voice.document",
      group_id: "group-a",
      data: {
        action: "archive",
        document_id: "doc-a",
        document_path: "docs/voice-secretary/archive/a.md",
        archived_from_document_path: "docs/voice-secretary/a.md",
      },
    } as LedgerEvent;
    const canonicalEvent = {
      kind: "assistant.voice.document",
      group_id: "group-a",
      data: {
        action: "archived",
        document: {
          document_id: "doc-b",
          document_path: "docs/voice-secretary/b.md",
          status: "archived",
        },
      },
    } as LedgerEvent;

    expect(archivedVoiceDocumentFromEvent(legacyEvent)).toMatchObject({
      document_id: "doc-a",
      document_path: "docs/voice-secretary/a.md",
    });
    expect(archivedVoiceDocumentFromEvent(canonicalEvent)).toMatchObject({
      document_id: "doc-b",
      document_path: "docs/voice-secretary/b.md",
    });

    const clearReferences = vi.fn();
    clearArchivedVoiceDocumentReference(clearReferences, legacyEvent, "fallback-group");
    expect(clearReferences).toHaveBeenCalledWith(
      "group-a",
      expect.objectContaining({ document_id: "doc-a" }),
    );
  });

  it("clears every document removed by a workspace refresh", () => {
    const clearReferences = vi.fn();
    clearRemovedVoiceDocumentReferences(
      clearReferences,
      "group-a",
      [
        { document_id: "doc-a", document_path: "voice/a.md", status: "active" },
        { document_id: "doc-b", document_path: "voice/b.md", status: "active" },
      ],
      [{ document_id: "doc-a", document_path: "voice/a.md", status: "active" }],
    );

    expect(clearReferences).toHaveBeenCalledOnce();
    expect(clearReferences).toHaveBeenCalledWith(
      "group-a",
      expect.objectContaining({ document_id: "doc-b" }),
    );
  });

  it("clears references immediately after archive success regardless of the visible group", async () => {
    const clearReferences = vi.fn();
    const archiveDocument = vi.fn(async () => ({
      ok: true as const,
      result: {
        group_id: "group-a",
        document: { document_id: "doc-a", document_path: "archive/a.md", status: "archived" },
      },
    }));

    await archiveVoiceDocumentWithReferenceCleanup({
      groupId: "group-a",
      documentPath: "voice/a.md",
      fallbackDocument: { document_id: "doc-a", document_path: "voice/a.md", status: "active" },
      clearReferences,
      archiveDocument,
    });

    expect(clearReferences).toHaveBeenCalledWith(
      "group-a",
      expect.objectContaining({ document_id: "doc-a", status: "archived" }),
    );
  });

  it("clears both pre-archive and archived identities for path-only legacy references", async () => {
    const clearReferences = vi.fn();
    const fallbackDocument = {
      document_id: "doc-a",
      document_path: "docs/voice-secretary/a.md",
      title: "A",
      status: "active",
    };
    const archivedDocument = {
      ...fallbackDocument,
      document_path: "docs/voice-secretary/archive/a.md",
      status: "archived",
    };
    const pathOnlyRef = {
      kind: "voice_document_ref" as const,
      group_id: "group-a",
      document_path: fallbackDocument.document_path,
    };

    expect(voiceDocumentRefMatchesDocument(pathOnlyRef, "group-a", archivedDocument)).toBe(false);
    expect(voiceDocumentRefMatchesDocument(pathOnlyRef, "group-a", fallbackDocument)).toBe(true);

    await archiveVoiceDocumentWithReferenceCleanup({
      groupId: "group-a",
      documentPath: fallbackDocument.document_path,
      fallbackDocument,
      clearReferences,
      archiveDocument: vi.fn(async () => ({
        ok: true as const,
        result: { group_id: "group-a", document: archivedDocument },
      })),
    });

    expect(clearReferences).toHaveBeenNthCalledWith(1, "group-a", fallbackDocument);
    expect(clearReferences).toHaveBeenNthCalledWith(2, "group-a", archivedDocument);
  });

  it("finds every new archive event even when a newer voice event follows it", () => {
    const baseline = [
      { id: "event-1", kind: "assistant.voice.document", data: { action: "saved" } },
    ] as LedgerEvent[];
    const initialized = newVoiceDocumentArchiveEvents(baseline, "");
    expect(initialized.events).toEqual([]);
    expect(initialized.cursor).toBe("event-1");

    const pythonArchive = {
      id: "event-2",
      kind: "assistant.voice.document",
      group_id: "group-a",
      data: {
        action: "archive",
        document_id: "doc-a",
        document_path: "docs/voice-secretary/archive/a.md",
        archived_from_document_path: "docs/voice-secretary/a.md",
      },
    } as LedgerEvent;
    const rustArchive = {
      id: "event-3",
      kind: "assistant.voice.document",
      group_id: "group-a",
      data: {
        action: "archived",
        document: {
          document_id: "doc-b",
          document_path: "docs/voice-secretary/b.md",
          status: "archived",
        },
      },
    } as LedgerEvent;
    const newerVoiceEvent = {
      id: "event-4",
      kind: "assistant.voice.session",
      group_id: "group-a",
      data: { action: "diarization_ready" },
    } as LedgerEvent;
    const projected = newVoiceDocumentArchiveEvents(
      [...baseline, pythonArchive, rustArchive, newerVoiceEvent],
      initialized.cursor,
    );

    expect(projected.events).toEqual([pythonArchive, rustArchive]);
    expect(projected.cursor).toBe("event-4");
  });
});
