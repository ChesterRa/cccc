import { useEffect, useMemo, useRef } from "react";
import type { archiveVoiceAssistantDocument } from "../../../services/api";
import { useGroupStore } from "../../../stores";
import { useComposerStore } from "../../../stores/useComposerStore";
import type { AssistantVoiceDocument, LedgerEvent } from "../../../types";
import { voiceDocumentPath } from "./voiceComposerUtils";

const EMPTY_LEDGER_EVENTS: LedgerEvent[] = [];

export type ClearVoiceDocumentReferences = (
  groupId: string,
  document: AssistantVoiceDocument,
) => void;

export function useVoiceDocumentReferenceCleanup(): ClearVoiceDocumentReferences {
  return useComposerStore((state) => state.clearQuotedVoiceDocumentRefsForDocument);
}

export function findRemovedVoiceDocuments(
  previous: AssistantVoiceDocument[],
  current: AssistantVoiceDocument[],
): AssistantVoiceDocument[] {
  const currentIds = new Set(
    current.map((document) => String(document.document_id || "").trim()).filter(Boolean),
  );
  const currentPaths = new Set(current.map(voiceDocumentPath).filter(Boolean));
  return previous.filter((document) => {
    const documentId = String(document.document_id || "").trim();
    if (documentId && currentIds.has(documentId)) return false;
    const documentPath = voiceDocumentPath(document);
    return !!documentPath && !currentPaths.has(documentPath);
  });
}

export function clearRemovedVoiceDocumentReferences(
  clearReferences: ClearVoiceDocumentReferences,
  groupId: string,
  previous: AssistantVoiceDocument[],
  current: AssistantVoiceDocument[],
): void {
  for (const document of findRemovedVoiceDocuments(previous, current)) {
    clearReferences(groupId, document);
  }
}

export function archivedVoiceDocumentFromEvent(
  event: LedgerEvent | null | undefined,
): AssistantVoiceDocument | null {
  if (String(event?.kind || "").trim() !== "assistant.voice.document") return null;
  const data =
    event?.data && typeof event.data === "object" ? (event.data as Record<string, unknown>) : {};
  const action = String(data.action || "")
    .trim()
    .toLowerCase();
  const status = String(data.status || "")
    .trim()
    .toLowerCase();
  if (!(["archive", "archived"].includes(action) || status === "archived")) return null;
  const nested =
    data.document && typeof data.document === "object"
      ? (data.document as Record<string, unknown>)
      : {};
  const documentId = String(nested.document_id || data.document_id || "").trim();
  const documentPath = String(
    nested.archived_from_workspace_path ||
      data.archived_from_document_path ||
      nested.document_path ||
      data.document_path ||
      "",
  ).trim();
  if (!documentId && !documentPath) return null;
  return {
    document_id: documentId,
    document_path: documentPath,
    workspace_path: String(nested.workspace_path || data.workspace_path || "").trim(),
    title: String(nested.title || data.title || "").trim(),
    status: "archived",
  };
}

export function clearArchivedVoiceDocumentReference(
  clearReferences: ClearVoiceDocumentReferences,
  event: LedgerEvent,
  fallbackGroupId: string,
): void {
  const document = archivedVoiceDocumentFromEvent(event);
  if (!document) return;
  clearReferences(String(event.group_id || fallbackGroupId || "").trim(), document);
}

export function newVoiceDocumentArchiveEvents(
  events: LedgerEvent[],
  afterEventId: string,
): { events: LedgerEvent[]; cursor: string } {
  const identifiedEvents = events.filter((event) => String(event.id || "").trim());
  const cursor = String(identifiedEvents.at(-1)?.id || afterEventId || "").trim();
  if (!identifiedEvents.length) return { events: [], cursor };

  const previousCursor = String(afterEventId || "").trim();
  if (!previousCursor) {
    let latest: LedgerEvent | undefined;
    for (let index = identifiedEvents.length - 1; index >= 0; index -= 1) {
      const event = identifiedEvents[index];
      if (
        !String(event.kind || "")
          .trim()
          .startsWith("assistant.voice.")
      )
        continue;
      latest = event;
      break;
    }
    return { events: latest && archivedVoiceDocumentFromEvent(latest) ? [latest] : [], cursor };
  }

  const previousIndex = identifiedEvents.findIndex(
    (event) => String(event.id || "").trim() === previousCursor,
  );
  const candidates = identifiedEvents.slice(previousIndex >= 0 ? previousIndex + 1 : 0);
  return { events: candidates.filter((event) => !!archivedVoiceDocumentFromEvent(event)), cursor };
}

export function useVoiceDocumentArchiveEventProjection({
  groupId,
  events,
  clearReferences,
}: {
  groupId: string;
  events: LedgerEvent[];
  clearReferences: ClearVoiceDocumentReferences;
}): void {
  const cursorByGroupRef = useRef(new Map<string, string>());

  useEffect(() => {
    const gid = String(groupId || "").trim();
    if (!gid || !events.length) return;
    const projected = newVoiceDocumentArchiveEvents(
      events,
      cursorByGroupRef.current.get(gid) || "",
    );
    if (projected.cursor) cursorByGroupRef.current.set(gid, projected.cursor);
    for (const event of projected.events) {
      clearArchivedVoiceDocumentReference(clearReferences, event, gid);
    }
  }, [clearReferences, events, groupId]);
}

export function useVoiceDocumentLedgerProjection(
  groupId: string,
  clearReferences: ClearVoiceDocumentReferences,
): LedgerEvent | null {
  const events = useGroupStore((state): LedgerEvent[] => {
    const gid = String(groupId || "").trim();
    return gid ? state.chatByGroup[gid]?.events || EMPTY_LEDGER_EVENTS : EMPTY_LEDGER_EVENTS;
  });
  useVoiceDocumentArchiveEventProjection({ groupId, events, clearReferences });
  return useMemo(() => {
    for (let index = events.length - 1; index >= 0; index -= 1) {
      const event = events[index];
      if (
        String(event?.kind || "")
          .trim()
          .startsWith("assistant.voice.")
      )
        return event;
    }
    return null;
  }, [events]);
}

export async function archiveVoiceDocumentWithReferenceCleanup({
  groupId,
  documentPath,
  fallbackDocument,
  clearReferences,
  archiveDocument,
}: {
  groupId: string;
  documentPath: string;
  fallbackDocument: AssistantVoiceDocument;
  clearReferences: ClearVoiceDocumentReferences;
  archiveDocument: typeof archiveVoiceAssistantDocument;
}) {
  const response = await archiveDocument(groupId, documentPath, { by: "user" });
  if (response.ok) {
    clearReferences(groupId, fallbackDocument);
    if (response.result.document) clearReferences(groupId, response.result.document);
  }
  return response;
}
