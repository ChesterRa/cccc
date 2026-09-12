import type { AssistantVoiceDocument, AssistantVoiceTranscriptSegmentResult } from "../../types";
import { apiJson, ApiResponse, asOptionalString, asRecord, asString } from "./base";
import { appendVoiceAssistantTranscriptSegment } from "./groups";

function normalizeVoiceDocument(value: unknown): AssistantVoiceDocument | null {
  const record = asRecord(value);
  if (!record) return null;
  const documentId = asString(record.document_id).trim();
  const documentPath =
    asOptionalString(record.document_path) || asOptionalString(record.workspace_path) || undefined;
  if (!documentId && !documentPath) return null;
  return {
    document_id: documentId || String(documentPath || ""),
    document_path: documentPath,
    filename: asOptionalString(record.filename) || undefined,
    assistant_id: asOptionalString(record.assistant_id) || undefined,
    title: asString(record.title).trim() || String(documentPath || "Untitled document"),
    status: asString(record.status).trim() || "active",
    storage_kind: asOptionalString(record.storage_kind) || undefined,
    workspace_path: asOptionalString(record.workspace_path) || undefined,
    content: asOptionalString(record.content) || undefined,
    content_sha256: asOptionalString(record.content_sha256) || undefined,
    content_chars: Number.isFinite(Number(record.content_chars))
      ? Number(record.content_chars)
      : undefined,
    revision_count: Number.isFinite(Number(record.revision_count))
      ? Number(record.revision_count)
      : undefined,
    source_segment_count: Number.isFinite(Number(record.source_segment_count))
      ? Number(record.source_segment_count)
      : undefined,
    last_source_segment_id: asOptionalString(record.last_source_segment_id) || undefined,
    last_source_path: asOptionalString(record.last_source_path) || undefined,
    created_at: asOptionalString(record.created_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
    created_by: asOptionalString(record.created_by) || undefined,
  };
}

export async function fetchVoiceAssistantDocumentContent(
  groupId: string,
  documentPath: string,
): Promise<ApiResponse<{ group_id: string; document?: AssistantVoiceDocument }>> {
  const gid = String(groupId || "").trim();
  const path = String(documentPath || "").trim();
  const params = new URLSearchParams();
  params.set("include_content", "true");
  params.set("include_documents_by_id", "false");
  params.set("include_documents_by_path", "false");
  if (path) params.set("document_path", path);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/documents?${params.toString()}`,
  );
  if (!resp.ok) return resp as ApiResponse<{ group_id: string; document?: AssistantVoiceDocument }>;
  const result = asRecord(resp.result) ?? {};
  const documents = Array.isArray(result.documents)
    ? result.documents
        .map((item) => normalizeVoiceDocument(item))
        .filter((item): item is AssistantVoiceDocument => !!item)
    : [];
  return {
    ok: true,
    result: { group_id: asString(result.group_id).trim() || gid, document: documents[0] },
  };
}

export async function retryVoiceAssistantTranscriptPersistence(
  groupId: string,
  payload: {
    sessionId: string;
    documentPath: string;
    text: string;
    language: string;
    modelId?: string;
    recognitionBackend?: string;
    partial?: boolean;
    pendingSegments?: unknown;
  },
): Promise<ApiResponse<AssistantVoiceTranscriptSegmentResult>> {
  const modelId = String(payload.modelId || "").trim();
  const pending = payload.pendingSegments ?? [];
  if (
    !Array.isArray(pending) ||
    pending.some((value) => {
      const segment = asRecord(value);
      return (
        !segment ||
        typeof segment.segment_id !== "string" ||
        !segment.segment_id.trim() ||
        typeof segment.text !== "string" ||
        !segment.text.trim() ||
        typeof segment.start_ms !== "number" ||
        !Number.isFinite(segment.start_ms) ||
        segment.start_ms < 0 ||
        typeof segment.end_ms !== "number" ||
        !Number.isFinite(segment.end_ms) ||
        segment.end_ms < segment.start_ms
      );
    })
  ) {
    return {
      ok: false,
      error: {
        code: "invalid_checkpoint_recovery",
        message: "Invalid transcript checkpoint recovery data",
      },
    };
  }
  let checkpoint: ApiResponse<AssistantVoiceTranscriptSegmentResult> = {
    ok: true,
    result: { group_id: groupId, session_id: payload.sessionId },
  };
  for (const [index, segment] of pending.entries()) {
    try {
      checkpoint = await appendVoiceAssistantTranscriptSegment(groupId, {
        sessionId: payload.sessionId,
        segmentId: segment.segment_id,
        documentPath: payload.documentPath,
        text: segment.text,
        language: payload.language,
        isFinal: true,
        flush: true,
        startMs: segment.start_ms,
        endMs: segment.end_ms,
        revision: { stage: "live", sourceModelId: modelId },
        trigger: {
          trigger_kind: "browser_checkpoint_retry",
          capture_mode: "service",
          recognition_backend: "external_provider_asr_streaming",
        },
        by: "user",
      });
    } catch (error) {
      // A response body can fail after fetch has resolved. Preserve the same
      // unconfirmed segment even when the transport helper cannot return JSON.
      checkpoint = {
        ok: false,
        error: {
          code: "checkpoint_retry_failed",
          message: error instanceof Error ? error.message : "Transcript checkpoint retry failed",
        },
      };
    }
    if (!checkpoint.ok) {
      return {
        ...checkpoint,
        error: {
          ...checkpoint.error,
          details: {
            ...asRecord(checkpoint.error.details),
            transcript_pending_segments: pending.slice(index),
          },
        },
      };
    }
  }
  // Partial provider output is recoverable live input, never a superseding final.
  if (payload.partial) return checkpoint;
  return appendVoiceAssistantTranscriptSegment(groupId, {
    sessionId: payload.sessionId,
    segmentId: "final-asr",
    documentPath: payload.documentPath,
    text: payload.text,
    language: payload.language,
    isFinal: true,
    flush: true,
    revision: {
      stage: "final",
      revisionOnly: true,
      supersedeStage: "live",
      sourceModelId: modelId,
    },
    trigger: {
      trigger_kind: "browser_persistence_retry",
      capture_mode: "service",
      recognition_backend:
        payload.recognitionBackend === "external_provider_asr_final"
          ? "external_provider_asr_final"
          : "assistant_service_local_asr_final",
      final_model_id: modelId,
    },
    by: "user",
  });
}
