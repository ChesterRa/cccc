import type { AssistantVoiceDocument, VoiceDocumentMessageRef } from "../types";

function trimString(value: unknown): string {
  return typeof value === "string" ? value.trim() : value == null ? "" : String(value).trim();
}

export function buildVoiceDocumentMessageRef(
  groupId: string,
  document: AssistantVoiceDocument,
): VoiceDocumentMessageRef | null {
  const normalizedGroupId = trimString(groupId);
  const documentPath = trimString(document.document_path || document.workspace_path);
  if (!normalizedGroupId || !documentPath) return null;

  const documentId = trimString(document.document_id);
  const title = trimString(document.title);
  return {
    kind: "voice_document_ref",
    v: 1,
    group_id: normalizedGroupId,
    document_path: documentPath,
    ...(documentId ? { document_id: documentId } : {}),
    ...(title ? { title } : {}),
  };
}

export function isVoiceDocumentMessageRef(value: unknown): value is VoiceDocumentMessageRef {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const record = value as Record<string, unknown>;
  return (
    trimString(record.kind) === "voice_document_ref" &&
    !!trimString(record.group_id) &&
    !!trimString(record.document_path)
  );
}

export function getVoiceDocumentMessageRefs(value: unknown): VoiceDocumentMessageRef[] {
  if (!Array.isArray(value)) return [];
  return value.filter(isVoiceDocumentMessageRef);
}

export function getVoiceDocumentRefLabel(ref: VoiceDocumentMessageRef): string {
  const title = trimString(ref.title);
  if (title) return title;
  const path = trimString(ref.document_path).replace(/\\/g, "/");
  return path.split("/").filter(Boolean).pop() || path || "Document";
}

export function voiceDocumentRefMatchesDocument(
  ref: VoiceDocumentMessageRef,
  groupId: string,
  document: AssistantVoiceDocument,
): boolean {
  if (trimString(ref.group_id) !== trimString(groupId)) return false;
  const refDocumentId = trimString(ref.document_id);
  const documentId = trimString(document.document_id);
  if (refDocumentId && documentId && refDocumentId === documentId) return true;
  const documentPath = trimString(document.document_path || document.workspace_path);
  return !!documentPath && trimString(ref.document_path) === documentPath;
}
