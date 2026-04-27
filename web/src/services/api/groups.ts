import type {
  AssistantServiceModel,
  AssistantServiceRuntime,
  AssistantStateResult,
  AssistantVoiceDocument,
  AssistantVoiceDocumentMutationResult,
  AssistantVoiceAskFeedback,
  AssistantVoiceInputResult,
  AssistantVoiceMeetingSession,
  AssistantVoicePromptDraft,
  AssistantVoicePromptDraftMutationResult,
  AssistantVoiceTranscriptSegmentResult,
  AssistantVoiceTranscriptionResult,
  BuiltinAssistant,
  GroupDoc,
  GroupMeta,
  GroupPresentation,
  PresentationBrowserSurfaceState,
  PresentationCard,
  PresentationRefSnapshot,
  PresentationWorkspaceItem,
  PresentationWorkspaceListing,
} from "../../types";
import {
  apiForm,
  apiJson,
  asOptionalString,
  asRecord,
  asString,
  asStringArray,
  ApiResponse,
  clearActorsReadOnlyRequest,
  clearGroupsReadRequest,
  groupPromptsRequestKey,
  groupPromptsRequestKey as groupPromptsKey,
  groupsRequestKey,
  normalizePresentation,
  normalizePresentationBrowserSurfaceState,
  normalizePresentationCard,
  petPeerContextRequestKey,
  RECENT_BOOTSTRAP_READ_TTL_MS,
  reuseRecentReadRequest,
  reuseSharedReadRequest,
  withAuthToken,
  clearSharedReadRequest,
} from "./base";

export async function fetchGroups() {
  return reuseRecentReadRequest(
    groupsRequestKey(),
    RECENT_BOOTSTRAP_READ_TTL_MS,
    () => apiJson<{ groups: GroupMeta[] }>("/api/v1/groups"),
  );
}

export async function fetchGroup(groupId: string, init?: RequestInit & { noCache?: boolean }) {
  return apiJson<{ group: GroupDoc }>(`/api/v1/groups/${encodeURIComponent(groupId)}`, init);
}

export function assistantStateRequestKey(groupId: string): string {
  return `assistants:${String(groupId || "").trim()}`;
}

function normalizeBuiltinAssistant(value: unknown): BuiltinAssistant | null {
  const record = asRecord(value);
  if (!record) return null;
  const assistantId = asString(record.assistant_id).trim();
  if (!assistantId) return null;
  const policyRecord = asRecord(record.policy);
  return {
    assistant_id: assistantId,
    kind: asString(record.kind).trim() || assistantId,
    enabled: !!record.enabled,
    principal: asOptionalString(record.principal) || undefined,
    lifecycle: asString(record.lifecycle).trim() || "disabled",
    health: asRecord(record.health) ?? {},
    policy: policyRecord
      ? {
          action_allowlist: asStringArray(policyRecord.action_allowlist),
          requires_user_confirmation: asStringArray(policyRecord.requires_user_confirmation),
        }
      : undefined,
    config: asRecord(record.config) ?? {},
    ui: asRecord(record.ui) ?? {},
  };
}

function normalizeAssistantVoiceDocument(value: unknown): AssistantVoiceDocument | null {
  const record = asRecord(value);
  if (!record) return null;
  const documentId = asString(record.document_id).trim();
  const documentPath = asOptionalString(record.document_path) || asOptionalString(record.workspace_path) || documentId;
  if (!documentId && !documentPath) return null;
  return {
    document_id: documentId || documentPath,
    document_path: documentPath,
    filename: asOptionalString(record.filename) || undefined,
    assistant_id: asOptionalString(record.assistant_id) || undefined,
    title: asOptionalString(record.title) || "Untitled document",
    status: asOptionalString(record.status) || "active",
    storage_kind: asOptionalString(record.storage_kind) || undefined,
    workspace_path: asOptionalString(record.workspace_path) || undefined,
    content: asOptionalString(record.content) || undefined,
    content_sha256: asOptionalString(record.content_sha256) || undefined,
    content_chars: Number.isFinite(Number(record.content_chars)) ? Number(record.content_chars) : undefined,
    revision_count: Number.isFinite(Number(record.revision_count)) ? Number(record.revision_count) : undefined,
    source_segment_count: Number.isFinite(Number(record.source_segment_count)) ? Number(record.source_segment_count) : undefined,
    last_source_segment_id: asOptionalString(record.last_source_segment_id) || undefined,
    last_source_path: asOptionalString(record.last_source_path) || undefined,
    created_at: asOptionalString(record.created_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
    created_by: asOptionalString(record.created_by) || undefined,
  };
}

function normalizeAssistantVoicePromptDraft(value: unknown): AssistantVoicePromptDraft | undefined {
  const record = asRecord(value);
  if (!record) return undefined;
  const requestId = asString(record.request_id).trim();
  const draftText = asString(record.draft_text).trim();
  if (!requestId || !draftText) return undefined;
  return {
    request_id: requestId,
    status: asOptionalString(record.status) || "pending",
    operation: asOptionalString(record.operation) || undefined,
    draft_text: draftText,
    draft_preview: asOptionalString(record.draft_preview) || undefined,
    summary: asOptionalString(record.summary) || undefined,
    composer_snapshot_hash: asOptionalString(record.composer_snapshot_hash) || undefined,
    created_at: asOptionalString(record.created_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
  };
}

function normalizeAssistantVoiceAskFeedback(value: unknown): AssistantVoiceAskFeedback | undefined {
  const record = asRecord(value);
  if (!record) return undefined;
  const requestId = asString(record.request_id).trim();
  if (!requestId) return undefined;
  return {
    request_id: requestId,
    status: asOptionalString(record.status) || "pending",
    request_text: asOptionalString(record.request_text) || undefined,
    request_preview: asOptionalString(record.request_preview) || undefined,
    reply_text: asOptionalString(record.reply_text) || undefined,
    document_path: asOptionalString(record.document_path) || undefined,
    artifact_paths: asStringArray(record.artifact_paths),
    source_summary: asOptionalString(record.source_summary) || undefined,
    checked_at: asOptionalString(record.checked_at) || undefined,
    source_urls: asStringArray(record.source_urls),
    target_kind: asOptionalString(record.target_kind) || undefined,
    intent_hint: asOptionalString(record.intent_hint) || undefined,
    language: asOptionalString(record.language) || undefined,
    handoff_target: asOptionalString(record.handoff_target) || undefined,
    handoff_request_id: asOptionalString(record.handoff_request_id) || undefined,
    target_actor_id: asOptionalString(record.target_actor_id) || undefined,
    created_at: asOptionalString(record.created_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
    cleared_at: asOptionalString(record.cleared_at) || undefined,
  };
}

function normalizeAssistantServiceModel(value: unknown): AssistantServiceModel | null {
  const record = asRecord(value);
  if (!record) return null;
  const modelId = asString(record.model_id).trim();
  if (!modelId && !asString(record.status).trim()) return null;
  const artifacts = Array.isArray(record.artifacts)
    ? record.artifacts
        .map((item) => {
          const artifact = asRecord(item);
          if (!artifact) return null;
          return {
            path: asOptionalString(artifact.path) || undefined,
            size_bytes: Number.isFinite(Number(artifact.size_bytes)) ? Number(artifact.size_bytes) : undefined,
          };
        })
        .filter((item) => item !== null)
    : undefined;
  return {
    model_id: modelId,
    kind: asOptionalString(record.kind) || undefined,
    runtime_id: asOptionalString(record.runtime_id) || undefined,
    title: asOptionalString(record.title) || undefined,
    description: asOptionalString(record.description) || undefined,
    status: asOptionalString(record.status) || undefined,
    available: typeof record.available === "boolean" ? record.available : undefined,
    installed: typeof record.installed === "boolean" ? record.installed : undefined,
    install_dir: asOptionalString(record.install_dir) || undefined,
    installed_at: asOptionalString(record.installed_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
    command_ready: typeof record.command_ready === "boolean" ? record.command_ready : undefined,
    streaming_ready: typeof record.streaming_ready === "boolean" ? record.streaming_ready : undefined,
    diarization_ready: typeof record.diarization_ready === "boolean" ? record.diarization_ready : undefined,
    streaming: asRecord(record.streaming) ?? undefined,
    diarization: asRecord(record.diarization) ?? undefined,
    manifest_sha256: asOptionalString(record.manifest_sha256) || undefined,
    downloaded_bytes: Number.isFinite(Number(record.downloaded_bytes)) ? Number(record.downloaded_bytes) : undefined,
    total_size_bytes: Number.isFinite(Number(record.total_size_bytes)) ? Number(record.total_size_bytes) : undefined,
    progress_percent: Number.isFinite(Number(record.progress_percent)) ? Number(record.progress_percent) : undefined,
    current_artifact_path: asOptionalString(record.current_artifact_path) || undefined,
    artifact_index: Number.isFinite(Number(record.artifact_index)) ? Number(record.artifact_index) : undefined,
    artifact_count: Number.isFinite(Number(record.artifact_count)) ? Number(record.artifact_count) : undefined,
    error: asRecord(record.error) ?? undefined,
    artifacts,
  };
}

function normalizeAssistantServiceRuntime(value: unknown): AssistantServiceRuntime | undefined {
  const record = asRecord(value);
  if (!record) return undefined;
  const runtimeId = asString(record.runtime_id).trim();
  if (!runtimeId && !asString(record.status).trim()) return undefined;
  const rawModules = asRecord(record.modules);
  const modules: Record<string, boolean> = {};
  if (rawModules) {
    for (const [key, value] of Object.entries(rawModules)) modules[key] = Boolean(value);
  }
  return {
    runtime_id: runtimeId,
    status: asOptionalString(record.status) || undefined,
    available: typeof record.available === "boolean" ? record.available : undefined,
    installed: typeof record.installed === "boolean" ? record.installed : undefined,
    install_dir: asOptionalString(record.install_dir) || undefined,
    python: asOptionalString(record.python) || undefined,
    packages: Array.isArray(record.packages) ? record.packages.map((item) => String(item || "")).filter(Boolean) : undefined,
    modules: Object.keys(modules).length > 0 ? modules : undefined,
    missing_modules: asStringArray(record.missing_modules),
    installed_at: asOptionalString(record.installed_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
    error: asRecord(record.error) ?? undefined,
  };
}

function normalizeAssistantVoiceMeetingSession(value: unknown): AssistantVoiceMeetingSession | undefined {
  const record = asRecord(value);
  if (!record) return undefined;
  const sessionId = asString(record.session_id).trim();
  if (!sessionId) return undefined;
  return {
    schema: Number.isFinite(Number(record.schema)) ? Number(record.schema) : undefined,
    group_id: asOptionalString(record.group_id) || undefined,
    session_id: sessionId,
    status: asOptionalString(record.status) || undefined,
    created_at: asOptionalString(record.created_at) || undefined,
    updated_at: asOptionalString(record.updated_at) || undefined,
    sample_rate: Number.isFinite(Number(record.sample_rate)) ? Number(record.sample_rate) : undefined,
    audio_duration_ms: Number.isFinite(Number(record.audio_duration_ms)) ? Number(record.audio_duration_ms) : undefined,
    language: asOptionalString(record.language) || undefined,
    document_path: asOptionalString(record.document_path) || undefined,
    latest_partial: asOptionalString(record.latest_partial) || undefined,
    last_final_text: asOptionalString(record.last_final_text) || undefined,
    diarization_ready: typeof record.diarization_ready === "boolean" ? record.diarization_ready : undefined,
    diarization_artifact_path: asOptionalString(record.diarization_artifact_path) || undefined,
    segments: Array.isArray(record.segments) ? record.segments.map((item) => asRecord(item)).filter((item): item is Record<string, unknown> => !!item) : [],
    diarization: asRecord(record.diarization) ?? undefined,
    error: asRecord(record.error),
  };
}

function normalizeAssistantStateResult(groupId: string, result: unknown): AssistantStateResult {
  const record = asRecord(result) ?? {};
  const assistants = Array.isArray(record.assistants)
    ? record.assistants
        .map((item) => normalizeBuiltinAssistant(item))
        .filter((item): item is BuiltinAssistant => !!item)
    : [];
  const assistantsById: Record<string, BuiltinAssistant> = {};
  for (const assistant of assistants) {
    assistantsById[assistant.assistant_id] = assistant;
  }
  const rawAssistantsById = asRecord(record.assistants_by_id);
  if (rawAssistantsById) {
    for (const value of Object.values(rawAssistantsById)) {
      const assistant = normalizeBuiltinAssistant(value);
      if (assistant) assistantsById[assistant.assistant_id] = assistant;
    }
  }
  const assistant = normalizeBuiltinAssistant(record.assistant);
  if (assistant) assistantsById[assistant.assistant_id] = assistant;
  const documents = Array.isArray(record.documents)
    ? record.documents
        .map((item) => normalizeAssistantVoiceDocument(item))
        .filter((item): item is AssistantVoiceDocument => !!item)
    : [];
  const documentsById: Record<string, AssistantVoiceDocument> = {};
  const documentsByPath: Record<string, AssistantVoiceDocument> = {};
  for (const document of documents) {
    documentsById[document.document_id] = document;
    if (document.document_path) documentsByPath[document.document_path] = document;
  }
  const rawDocumentsById = asRecord(record.documents_by_id);
  if (rawDocumentsById) {
    for (const value of Object.values(rawDocumentsById)) {
      const document = normalizeAssistantVoiceDocument(value);
      if (document) {
        documentsById[document.document_id] = document;
        if (document.document_path) documentsByPath[document.document_path] = document;
      }
    }
  }
  const rawDocumentsByPath = asRecord(record.documents_by_path);
  if (rawDocumentsByPath) {
    for (const value of Object.values(rawDocumentsByPath)) {
      const document = normalizeAssistantVoiceDocument(value);
      if (document) {
        documentsById[document.document_id] = document;
        if (document.document_path) documentsByPath[document.document_path] = document;
      }
    }
  }
  const askRequests = Array.isArray(record.ask_requests)
    ? record.ask_requests
        .map((item) => normalizeAssistantVoiceAskFeedback(item))
        .filter((item): item is AssistantVoiceAskFeedback => !!item)
    : [];
  const serviceModels = Array.isArray(record.service_models)
    ? record.service_models
        .map((item) => normalizeAssistantServiceModel(item))
        .filter((item): item is AssistantServiceModel => !!item)
    : [];
  const serviceModelsById: Record<string, AssistantServiceModel> = {};
  for (const model of serviceModels) {
    if (model.model_id) serviceModelsById[model.model_id] = model;
  }
  const rawServiceModelsById = asRecord(record.service_models_by_id);
  if (rawServiceModelsById) {
    for (const value of Object.values(rawServiceModelsById)) {
      const model = normalizeAssistantServiceModel(value);
      if (model?.model_id) serviceModelsById[model.model_id] = model;
    }
  }
  const serviceRuntimes = Array.isArray(record.service_runtimes)
    ? record.service_runtimes
        .map((item) => normalizeAssistantServiceRuntime(item))
        .filter((item): item is AssistantServiceRuntime => !!item)
    : [];
  const serviceRuntimesById: Record<string, AssistantServiceRuntime> = {};
  for (const runtime of serviceRuntimes) {
    if (runtime.runtime_id) serviceRuntimesById[runtime.runtime_id] = runtime;
  }
  const rawServiceRuntimesById = asRecord(record.service_runtimes_by_id);
  if (rawServiceRuntimesById) {
    for (const value of Object.values(rawServiceRuntimesById)) {
      const runtime = normalizeAssistantServiceRuntime(value);
      if (runtime?.runtime_id) serviceRuntimesById[runtime.runtime_id] = runtime;
    }
  }
  const primaryServiceRuntime = normalizeAssistantServiceRuntime(record.service_runtime);
  if (primaryServiceRuntime?.runtime_id) serviceRuntimesById[primaryServiceRuntime.runtime_id] = primaryServiceRuntime;
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistants: Object.values(assistantsById).sort((a, b) => a.assistant_id.localeCompare(b.assistant_id)),
    assistants_by_id: assistantsById,
    assistant: assistant || undefined,
    documents: Object.values(documentsById).sort((a, b) => {
      const aCreated = String(a.created_at || "");
      const bCreated = String(b.created_at || "");
      if (aCreated !== bCreated) return bCreated.localeCompare(aCreated);
      return String(b.document_id || "").localeCompare(String(a.document_id || ""));
    }),
    documents_by_id: documentsById,
    documents_by_path: documentsByPath,
    active_document_id: asOptionalString(record.active_document_id) || undefined,
    capture_target_document_id:
      asOptionalString(record.capture_target_document_id) || asOptionalString(record.active_document_id) || undefined,
    active_document_path: asOptionalString(record.active_document_path) || undefined,
    capture_target_document_path:
      asOptionalString(record.capture_target_document_path) || asOptionalString(record.active_document_path) || undefined,
    new_input_available: Boolean(record.new_input_available),
    prompt_draft: normalizeAssistantVoicePromptDraft(record.prompt_draft),
    ask_requests: askRequests,
    latest_ask_request: normalizeAssistantVoiceAskFeedback(record.latest_ask_request) || askRequests[0],
    service_models: [
      ...serviceModels,
      ...Object.values(serviceModelsById).filter((model) => !serviceModels.some((item) => item.model_id === model.model_id)),
    ],
    service_models_by_id: serviceModelsById,
    service_runtime: primaryServiceRuntime,
    service_runtimes: Object.values(serviceRuntimesById).sort((a, b) => a.runtime_id.localeCompare(b.runtime_id)),
    service_runtimes_by_id: serviceRuntimesById,
  };
}

type AssistantMutationResult = {
  group_id: string;
  assistant?: BuiltinAssistant;
  event?: unknown;
};

function normalizeAssistantVoiceTranscriptionResult(groupId: string, result: unknown): AssistantVoiceTranscriptionResult {
  const record = asRecord(result) ?? {};
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistant: normalizeBuiltinAssistant(record.assistant) || undefined,
    transcript: asString(record.transcript),
    mime_type: asOptionalString(record.mime_type) || undefined,
    language: asOptionalString(record.language) || undefined,
    bytes: Number.isFinite(Number(record.bytes)) ? Number(record.bytes) : undefined,
    backend: asOptionalString(record.backend) || undefined,
    service: asRecord(record.service) ?? undefined,
    asr: asRecord(record.asr) ?? undefined,
  };
}

function normalizeAssistantVoiceTranscriptSegmentResult(groupId: string, result: unknown): AssistantVoiceTranscriptSegmentResult {
  const record = asRecord(result) ?? {};
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistant: normalizeBuiltinAssistant(record.assistant) || undefined,
    session_id: asString(record.session_id),
    segment: asRecord(record.segment) ?? undefined,
    segment_path: asOptionalString(record.segment_path) || undefined,
    document: normalizeAssistantVoiceDocument(record.document) || undefined,
    document_updated: Boolean(record.document_updated),
    input_event: asRecord(record.input_event) ?? undefined,
    input_event_created: Boolean(record.input_event_created),
    input_notify_emitted: Boolean(record.input_notify_emitted),
    input_notify_error: asOptionalString(record.input_notify_error) || undefined,
    actor_woken: Boolean(record.actor_woken),
    actor_wake_error: asOptionalString(record.actor_wake_error) || undefined,
    actor_notify_delivered: Boolean(record.actor_notify_delivered),
    actor_notify_delivery_error: asOptionalString(record.actor_notify_delivery_error) || undefined,
  };
}

function normalizeAssistantMutationResult(groupId: string, result: unknown): AssistantMutationResult {
  const record = asRecord(result) ?? {};
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistant: normalizeBuiltinAssistant(record.assistant) || undefined,
    event: record.event,
  };
}

function normalizeAssistantVoiceDocumentMutationResult(groupId: string, result: unknown): AssistantVoiceDocumentMutationResult {
  const record = asRecord(result) ?? {};
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistant: normalizeBuiltinAssistant(record.assistant) || undefined,
    document: normalizeAssistantVoiceDocument(record.document) || undefined,
    input_event: asRecord(record.input_event) ?? undefined,
    input_event_created: Boolean(record.input_event_created),
    input_notify_emitted: Boolean(record.input_notify_emitted),
    input_notify_error: asOptionalString(record.input_notify_error) || undefined,
    actor_woken: Boolean(record.actor_woken),
    actor_wake_error: asOptionalString(record.actor_wake_error) || undefined,
    actor_notify_delivered: Boolean(record.actor_notify_delivered),
    actor_notify_delivery_error: asOptionalString(record.actor_notify_delivery_error) || undefined,
    event: record.event,
    request_id: asOptionalString(record.request_id) || undefined,
  };
}

function normalizeAssistantVoiceInputResult(groupId: string, result: unknown): AssistantVoiceInputResult {
  const record = asRecord(result) ?? {};
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistant: normalizeBuiltinAssistant(record.assistant) || undefined,
    document: normalizeAssistantVoiceDocument(record.document) || undefined,
    input_event: asRecord(record.input_event) ?? undefined,
    input_event_created: Boolean(record.input_event_created),
    input_notify_emitted: Boolean(record.input_notify_emitted),
    input_notify_error: asOptionalString(record.input_notify_error) || undefined,
    actor_woken: Boolean(record.actor_woken),
    actor_wake_error: asOptionalString(record.actor_wake_error) || undefined,
    actor_notify_delivered: Boolean(record.actor_notify_delivered),
    actor_notify_delivery_error: asOptionalString(record.actor_notify_delivery_error) || undefined,
    event: record.event,
    request_id: asOptionalString(record.request_id) || undefined,
  };
}

function normalizeAssistantVoicePromptDraftMutationResult(
  groupId: string,
  result: unknown,
): AssistantVoicePromptDraftMutationResult {
  const record = asRecord(result) ?? {};
  return {
    group_id: asString(record.group_id).trim() || groupId,
    assistant: normalizeBuiltinAssistant(record.assistant) || undefined,
    prompt_draft: normalizeAssistantVoicePromptDraft(record.prompt_draft),
    event: record.event,
  };
}

function clearAssistantStateRequest(groupId: string): void {
  clearSharedReadRequest(assistantStateRequestKey(groupId));
}

export async function fetchAssistantState(groupId: string): Promise<ApiResponse<AssistantStateResult>> {
  const gid = String(groupId || "").trim();
  return reuseSharedReadRequest(assistantStateRequestKey(gid), async () => {
    const resp = await apiJson<unknown>(`/api/v1/groups/${encodeURIComponent(gid)}/assistants`);
    if (!resp.ok) return resp as ApiResponse<AssistantStateResult>;
    return { ok: true, result: normalizeAssistantStateResult(gid, resp.result) };
  });
}

export async function fetchAssistant(
  groupId: string,
  assistantId: string,
  opts?: { promptRequestId?: string },
): Promise<ApiResponse<AssistantStateResult>> {
  const gid = String(groupId || "").trim();
  const aid = String(assistantId || "").trim();
  const params = new URLSearchParams();
  const promptRequestId = String(opts?.promptRequestId || "").trim();
  if (promptRequestId) params.set("prompt_request_id", promptRequestId);
  const query = params.toString();
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/${encodeURIComponent(aid)}${query ? `?${query}` : ""}`,
  );
  if (!resp.ok) return resp as ApiResponse<AssistantStateResult>;
  return { ok: true, result: normalizeAssistantStateResult(gid, resp.result) };
}

export async function updateAssistantSettings(
  groupId: string,
  assistantId: string,
  payload: { enabled?: boolean; config?: Record<string, unknown>; by?: string },
): Promise<ApiResponse<AssistantMutationResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/${encodeURIComponent(String(assistantId || "").trim())}/settings`,
    {
      method: "PUT",
      body: JSON.stringify({
        enabled: payload.enabled,
        config: payload.config,
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantMutationResult>;
  return { ok: true, result: normalizeAssistantMutationResult(gid, resp.result) };
}

export async function updateAssistantStatus(
  groupId: string,
  assistantId: string,
  payload: {
    lifecycle: BuiltinAssistant["lifecycle"];
    health?: Record<string, unknown>;
    by?: string;
  },
): Promise<ApiResponse<AssistantMutationResult>> {
  const gid = String(groupId || "").trim();
  const aid = String(assistantId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/${encodeURIComponent(aid)}/status`,
    {
      method: "POST",
      body: JSON.stringify({
        assistant_id: aid,
        lifecycle: payload.lifecycle,
        health: payload.health || {},
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantMutationResult>;
  return { ok: true, result: normalizeAssistantMutationResult(gid, resp.result) };
}

export async function transcribeVoiceAssistantAudio(
  groupId: string,
  payload: {
    audioBase64: string;
    mimeType: string;
    language?: string;
    by?: string;
  },
): Promise<ApiResponse<AssistantVoiceTranscriptionResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/transcriptions`,
    {
      method: "POST",
      body: JSON.stringify({
        audio_base64: String(payload.audioBase64 || ""),
        mime_type: String(payload.mimeType || "application/octet-stream"),
        language: String(payload.language || ""),
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceTranscriptionResult>;
  return { ok: true, result: normalizeAssistantVoiceTranscriptionResult(gid, resp.result) };
}

export async function fetchLatestVoiceAssistantMeetingSession(
  groupId: string,
  opts?: { documentPath?: string },
): Promise<ApiResponse<{ group_id: string; session?: AssistantVoiceMeetingSession }>> {
  const gid = String(groupId || "").trim();
  const params = new URLSearchParams();
  const documentPath = String(opts?.documentPath || "").trim();
  if (documentPath) params.set("document_path", documentPath);
  const query = params.toString();
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/sessions/latest${query ? `?${query}` : ""}`,
  );
  if (!resp.ok) return resp as ApiResponse<{ group_id: string; session?: AssistantVoiceMeetingSession }>;
  const record = asRecord(resp.result) ?? {};
  return {
    ok: true,
    result: {
      group_id: asString(record.group_id).trim() || gid,
      session: normalizeAssistantVoiceMeetingSession(record.session),
    },
  };
}

export async function installVoiceAssistantModel(
  groupId: string,
  payload: { modelId: string; by?: string; background?: boolean },
): Promise<ApiResponse<AssistantMutationResult & { model?: AssistantServiceModel; service_runtime?: AssistantServiceRuntime }>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/models/install`,
    {
      method: "POST",
      body: JSON.stringify({
        model_id: String(payload.modelId || "").trim(),
        by: String(payload.by || "user").trim() || "user",
        background: Boolean(payload.background),
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantMutationResult & { model?: AssistantServiceModel; service_runtime?: AssistantServiceRuntime }>;
  const normalized = normalizeAssistantMutationResult(gid, resp.result);
  const resultRecord = asRecord(resp.result) ?? {};
  return {
    ok: true,
    result: {
      ...normalized,
      model: normalizeAssistantServiceModel(resultRecord.model) || undefined,
      service_runtime: normalizeAssistantServiceRuntime(resultRecord.service_runtime),
    },
  };
}

export async function installVoiceAssistantRuntime(
  groupId: string,
  payload: { runtimeId?: string; by?: string; background?: boolean } = {},
): Promise<ApiResponse<AssistantMutationResult & { service_runtime?: AssistantServiceRuntime }>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/runtime/install`,
    {
      method: "POST",
      body: JSON.stringify({
        runtime_id: String(payload.runtimeId || "").trim(),
        by: String(payload.by || "user").trim() || "user",
        background: payload.background !== false,
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantMutationResult & { service_runtime?: AssistantServiceRuntime }>;
  const normalized = normalizeAssistantMutationResult(gid, resp.result);
  const resultRecord = asRecord(resp.result) ?? {};
  return {
    ok: true,
    result: {
      ...normalized,
      service_runtime: normalizeAssistantServiceRuntime(resultRecord.service_runtime),
    },
  };
}

export async function appendVoiceAssistantTranscriptSegment(
  groupId: string,
  payload: {
    sessionId: string;
    segmentId?: string;
    documentPath?: string;
    text?: string;
    language?: string;
    isFinal?: boolean;
    flush?: boolean;
    trigger?: Record<string, unknown>;
    startMs?: number;
    endMs?: number;
    speakerLabel?: string;
    by?: string;
  },
): Promise<ApiResponse<AssistantVoiceTranscriptSegmentResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/transcript_segments`,
    {
      method: "POST",
      body: JSON.stringify({
        session_id: String(payload.sessionId || "").trim(),
        segment_id: String(payload.segmentId || "").trim(),
        document_path: String(payload.documentPath || "").trim(),
        text: String(payload.text || ""),
        language: String(payload.language || ""),
        is_final: payload.isFinal !== false,
        flush: Boolean(payload.flush),
        trigger: payload.trigger || {},
        start_ms: Number.isFinite(Number(payload.startMs)) ? Math.max(0, Math.round(Number(payload.startMs))) : null,
        end_ms: Number.isFinite(Number(payload.endMs)) ? Math.max(0, Math.round(Number(payload.endMs))) : null,
        speaker_label: String(payload.speakerLabel || "").trim(),
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceTranscriptSegmentResult>;
  return { ok: true, result: normalizeAssistantVoiceTranscriptSegmentResult(gid, resp.result) };
}

export async function selectVoiceAssistantDocument(
  groupId: string,
  documentPath: string,
  payload: { by?: string } = {},
): Promise<ApiResponse<AssistantVoiceDocumentMutationResult>> {
  const gid = String(groupId || "").trim();
  const docPath = String(documentPath || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/documents/select`,
    {
      method: "POST",
      body: JSON.stringify({
        document_path: docPath,
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceDocumentMutationResult>;
  return { ok: true, result: normalizeAssistantVoiceDocumentMutationResult(gid, resp.result) };
}

export async function saveVoiceAssistantDocument(
  groupId: string,
  payload: {
    documentPath?: string;
    title?: string;
    content: string;
    status?: string;
    createNew?: boolean;
    by?: string;
  },
): Promise<ApiResponse<AssistantVoiceDocumentMutationResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const body: Record<string, unknown> = {
    document_path: String(payload.documentPath || "").trim(),
    status: String(payload.status || ""),
    create_new: Boolean(payload.createNew),
    by: String(payload.by || "user").trim() || "user",
  };
  if (payload.title !== undefined) {
    body.title = String(payload.title || "");
  }
  if (!payload.createNew || String(payload.content || "").trim()) {
    body.content = String(payload.content || "");
  }
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/documents`,
    {
      method: "PUT",
      body: JSON.stringify(body),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceDocumentMutationResult>;
  return { ok: true, result: normalizeAssistantVoiceDocumentMutationResult(gid, resp.result) };
}

export async function sendVoiceAssistantDocumentInstruction(
  groupId: string,
  documentPath: string,
  payload: {
    instruction: string;
    sourceText?: string;
    documentPath?: string;
    trigger?: Record<string, unknown>;
    by?: string;
  },
): Promise<ApiResponse<AssistantVoiceDocumentMutationResult>> {
  const gid = String(groupId || "").trim();
  const docPath = String(documentPath || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/documents/instructions`,
    {
      method: "POST",
      body: JSON.stringify({
        document_path: String(payload.documentPath || docPath).trim(),
        instruction: String(payload.instruction || ""),
        source_text: String(payload.sourceText || ""),
        trigger: payload.trigger || {},
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceDocumentMutationResult>;
  return { ok: true, result: normalizeAssistantVoiceDocumentMutationResult(gid, resp.result) };
}

export async function appendVoiceAssistantInput(
  groupId: string,
  payload: {
    kind: "voice_instruction" | "prompt_refine";
    text?: string;
    instruction?: string;
    sourceText?: string;
    documentPath?: string;
    voiceTranscript?: string;
    composerText?: string;
    requestId?: string;
    operation?: string;
    composerContext?: Record<string, unknown>;
    composerSnapshotHash?: string;
    language?: string;
    trigger?: Record<string, unknown>;
    by?: string;
  },
): Promise<ApiResponse<AssistantVoiceInputResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/inputs`,
    {
      method: "POST",
      body: JSON.stringify({
        kind: String(payload.kind || ""),
        text: String(payload.text || ""),
        instruction: String(payload.instruction || ""),
        source_text: String(payload.sourceText || ""),
        document_path: String(payload.documentPath || "").trim(),
        voice_transcript: String(payload.voiceTranscript || ""),
        composer_text: String(payload.composerText || ""),
        request_id: String(payload.requestId || ""),
        operation: String(payload.operation || ""),
        composer_context: payload.composerContext || {},
        composer_snapshot_hash: String(payload.composerSnapshotHash || ""),
        language: String(payload.language || ""),
        trigger: payload.trigger || {},
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceInputResult>;
  return { ok: true, result: normalizeAssistantVoiceInputResult(gid, resp.result) };
}

export async function ackVoiceAssistantPromptDraft(
  groupId: string,
  payload: { requestId: string; status: "applied" | "dismissed" | "stale"; by?: string },
): Promise<ApiResponse<AssistantVoicePromptDraftMutationResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/prompt_drafts/ack`,
    {
      method: "POST",
      body: JSON.stringify({
        request_id: String(payload.requestId || "").trim(),
        status: payload.status,
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoicePromptDraftMutationResult>;
  return { ok: true, result: normalizeAssistantVoicePromptDraftMutationResult(gid, resp.result) };
}

export async function clearVoiceAssistantAskRequests(
  groupId: string,
  payload: { keepActive?: boolean; by?: string } = {},
): Promise<ApiResponse<AssistantStateResult>> {
  const gid = String(groupId || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/ask_requests/clear`,
    {
      method: "POST",
      body: JSON.stringify({
        keep_active: Boolean(payload.keepActive),
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantStateResult>;
  return { ok: true, result: normalizeAssistantStateResult(gid, resp.result) };
}

export async function archiveVoiceAssistantDocument(
  groupId: string,
  documentPath: string,
  payload: { by?: string } = {},
): Promise<ApiResponse<AssistantVoiceDocumentMutationResult>> {
  const gid = String(groupId || "").trim();
  const docPath = String(documentPath || "").trim();
  clearAssistantStateRequest(gid);
  const resp = await apiJson<unknown>(
    `/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/documents/archive`,
    {
      method: "POST",
      body: JSON.stringify({
        document_path: docPath,
        by: String(payload.by || "user").trim() || "user",
      }),
    },
  );
  clearAssistantStateRequest(gid);
  if (!resp.ok) return resp as ApiResponse<AssistantVoiceDocumentMutationResult>;
  return { ok: true, result: normalizeAssistantVoiceDocumentMutationResult(gid, resp.result) };
}

export async function fetchPresentation(
  groupId: string,
): Promise<ApiResponse<{ group_id: string; presentation: GroupPresentation }>> {
  const resp = await apiJson<{ group_id?: string; presentation?: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation`,
  );
  if (!resp.ok) return resp as ApiResponse<{ group_id: string; presentation: GroupPresentation }>;
  return {
    ok: true,
    result: {
      group_id: asString(resp.result.group_id).trim() || groupId,
      presentation: normalizePresentation(resp.result.presentation),
    },
  };
}

type PresentationMutationResult = {
  group_id: string;
  slot_id?: string;
  cleared_slots?: string[];
  card?: PresentationCard;
  presentation: GroupPresentation;
};

function normalizePresentationMutationResult(
  groupId: string,
  result: { group_id?: unknown; slot_id?: unknown; cleared_slots?: unknown; card?: unknown; presentation?: unknown },
): PresentationMutationResult {
  return {
    group_id: asString(result.group_id).trim() || groupId,
    slot_id: asOptionalString(result.slot_id) || undefined,
    cleared_slots: Array.isArray(result.cleared_slots)
      ? result.cleared_slots.map((slot) => asString(slot).trim()).filter(Boolean)
      : undefined,
    card: normalizePresentationCard(result.card) || undefined,
    presentation: normalizePresentation(result.presentation),
  };
}

export async function publishPresentationUrl(
  groupId: string,
  payload: { slotId: string; url: string; title?: string; summary?: string },
): Promise<ApiResponse<PresentationMutationResult>> {
  const resp = await apiJson<{
    group_id?: unknown;
    slot_id?: unknown;
    card?: unknown;
    presentation?: unknown;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/presentation/publish`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      slot: String(payload.slotId || "").trim() || "auto",
      url: String(payload.url || "").trim(),
      title: String(payload.title || "").trim(),
      summary: String(payload.summary || "").trim(),
    }),
  });
  if (!resp.ok) return resp as ApiResponse<PresentationMutationResult>;
  return { ok: true, result: normalizePresentationMutationResult(groupId, resp.result) };
}

export async function publishPresentationUpload(
  groupId: string,
  payload: { slotId: string; file: File; title?: string; summary?: string },
): Promise<ApiResponse<PresentationMutationResult>> {
  const form = new FormData();
  form.append("by", "user");
  form.append("slot", String(payload.slotId || "").trim() || "auto");
  form.append("title", String(payload.title || "").trim());
  form.append("summary", String(payload.summary || "").trim());
  form.append("file", payload.file);
  const resp = await apiForm<{
    group_id?: unknown;
    slot_id?: unknown;
    card?: unknown;
    presentation?: unknown;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/presentation/publish_upload`, form);
  if (!resp.ok) return resp as ApiResponse<PresentationMutationResult>;
  return { ok: true, result: normalizePresentationMutationResult(groupId, resp.result) };
}

export async function publishPresentationWorkspace(
  groupId: string,
  payload: { slotId: string; path: string; title?: string; summary?: string },
): Promise<ApiResponse<PresentationMutationResult>> {
  const resp = await apiJson<{
    group_id?: unknown;
    slot_id?: unknown;
    card?: unknown;
    presentation?: unknown;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/presentation/publish_workspace`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      slot: String(payload.slotId || "").trim() || "auto",
      path: String(payload.path || "").trim(),
      title: String(payload.title || "").trim(),
      summary: String(payload.summary || "").trim(),
    }),
  });
  if (!resp.ok) return resp as ApiResponse<PresentationMutationResult>;
  return { ok: true, result: normalizePresentationMutationResult(groupId, resp.result) };
}

export async function clearPresentationSlot(
  groupId: string,
  slotId: string,
): Promise<ApiResponse<PresentationMutationResult>> {
  const resp = await apiJson<{
    group_id?: unknown;
    cleared_slots?: unknown;
    presentation?: unknown;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/presentation/clear`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      slot: String(slotId || "").trim(),
    }),
  });
  if (!resp.ok) return resp as ApiResponse<PresentationMutationResult>;
  return { ok: true, result: normalizePresentationMutationResult(groupId, resp.result) };
}

function normalizePresentationWorkspaceItem(value: unknown): PresentationWorkspaceItem | null {
  const record = asRecord(value);
  if (!record) return null;
  const name = asString(record.name).trim();
  const path = asString(record.path).trim();
  if (!name || !path) return null;
  return {
    name,
    path,
    is_dir: !!record.is_dir,
    mime_type: asOptionalString(record.mime_type),
  };
}

export async function fetchPresentationWorkspaceListing(
  groupId: string,
  path = "",
): Promise<ApiResponse<PresentationWorkspaceListing>> {
  const params = new URLSearchParams();
  if (String(path || "").trim()) params.set("path", String(path).trim());
  const suffix = params.toString() ? `?${params.toString()}` : "";
  const resp = await apiJson<{
    root_path?: unknown;
    path?: unknown;
    parent?: unknown;
    items?: unknown;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/presentation/workspace/list${suffix}`);
  if (!resp.ok) return resp as ApiResponse<PresentationWorkspaceListing>;
  const items = Array.isArray(resp.result.items)
    ? resp.result.items
        .map((item) => normalizePresentationWorkspaceItem(item))
        .filter((item): item is PresentationWorkspaceItem => !!item)
    : [];
  return {
    ok: true,
    result: {
      root_path: asString(resp.result.root_path).trim(),
      path: asString(resp.result.path).trim(),
      parent:
        typeof resp.result.parent === "string"
          ? String(resp.result.parent)
          : resp.result.parent == null
            ? null
            : String(resp.result.parent),
      items,
    },
  };
}

export function getPresentationAssetUrl(groupId: string, slotId: string, cacheBust?: string | number): string {
  const base = withAuthToken(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/slots/${encodeURIComponent(slotId)}/asset`,
  );
  if (cacheBust === undefined || cacheBust === null || cacheBust === "") return base;
  const separator = base.includes("?") ? "&" : "?";
  return `${base}${separator}v=${encodeURIComponent(String(cacheBust))}`;
}

export function getGroupBlobUrl(groupId: string, relPath: string): string {
  const normalized = String(relPath || "").trim();
  if (!normalized.startsWith("state/blobs/")) return "";
  const blobName = normalized.split("/").pop() || "";
  if (!blobName) return "";
  return withAuthToken(`/api/v1/groups/${encodeURIComponent(groupId)}/blobs/${encodeURIComponent(blobName)}`);
}

export async function fetchPresentationBrowserSurfaceSession(
  groupId: string,
  slotId: string,
): Promise<ApiResponse<{ group_id: string; browser_surface: PresentationBrowserSurfaceState }>> {
  const resp = await apiJson<{ group_id?: unknown; browser_surface?: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/browser_surface/session?slot=${encodeURIComponent(slotId)}`,
  );
  if (!resp.ok) return resp as ApiResponse<{ group_id: string; browser_surface: PresentationBrowserSurfaceState }>;
  return {
    ok: true,
    result: {
      group_id: asString(resp.result.group_id).trim() || groupId,
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export async function startPresentationBrowserSurfaceSession(
  groupId: string,
  payload: { slotId: string; url: string; width?: number; height?: number },
): Promise<ApiResponse<{ group_id: string; browser_surface: PresentationBrowserSurfaceState }>> {
  const resp = await apiJson<{ group_id?: unknown; browser_surface?: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/browser_surface/session`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        slot: String(payload.slotId || "").trim(),
        url: String(payload.url || "").trim(),
        width: Number.isFinite(Number(payload.width)) ? Number(payload.width) : 1280,
        height: Number.isFinite(Number(payload.height)) ? Number(payload.height) : 800,
      }),
    },
  );
  if (!resp.ok) return resp as ApiResponse<{ group_id: string; browser_surface: PresentationBrowserSurfaceState }>;
  return {
    ok: true,
    result: {
      group_id: asString(resp.result.group_id).trim() || groupId,
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export async function uploadPresentationReferenceSnapshot(
  groupId: string,
  payload: {
    slotId: string;
    file: File;
    source?: string;
    capturedAt?: string;
    width?: number;
    height?: number;
  },
): Promise<ApiResponse<{ group_id: string; snapshot: PresentationRefSnapshot }>> {
  const form = new FormData();
  form.append("by", "user");
  form.append("slot", String(payload.slotId || "").trim());
  form.append("source", String(payload.source || "").trim() || "browser_surface");
  form.append("captured_at", String(payload.capturedAt || "").trim());
  form.append("width", String(Number.isFinite(Number(payload.width)) ? Number(payload.width) : 0));
  form.append("height", String(Number.isFinite(Number(payload.height)) ? Number(payload.height) : 0));
  form.append("file", payload.file);
  const resp = await apiForm<{ group_id?: unknown; snapshot?: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/ref_snapshot`,
    form,
  );
  if (!resp.ok) return resp as ApiResponse<{ group_id: string; snapshot: PresentationRefSnapshot }>;
  const snapshot = asRecord(resp.result.snapshot);
  return {
    ok: true,
    result: {
      group_id: asString(resp.result.group_id).trim() || groupId,
      snapshot: {
        path: asString(snapshot?.path).trim(),
        mime_type: asOptionalString(snapshot?.mime_type) || undefined,
        bytes: Number.isFinite(Number(snapshot?.bytes)) ? Number(snapshot?.bytes) : undefined,
        sha256: asOptionalString(snapshot?.sha256) || undefined,
        width: Number.isFinite(Number(snapshot?.width)) ? Number(snapshot?.width) : undefined,
        height: Number.isFinite(Number(snapshot?.height)) ? Number(snapshot?.height) : undefined,
        captured_at: asOptionalString(snapshot?.captured_at) || undefined,
        source: asOptionalString(snapshot?.source) || undefined,
      },
    },
  };
}

export async function closePresentationBrowserSurfaceSession(
  groupId: string,
  slotId: string,
): Promise<ApiResponse<{ group_id: string; closed: boolean; browser_surface: PresentationBrowserSurfaceState }>> {
  const resp = await apiJson<{ group_id?: unknown; closed?: unknown; browser_surface?: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/browser_surface/session/close`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", slot: String(slotId || "").trim() }),
    },
  );
  if (!resp.ok) {
    return resp as ApiResponse<{ group_id: string; closed: boolean; browser_surface: PresentationBrowserSurfaceState }>;
  }
  return {
    ok: true,
    result: {
      group_id: asString(resp.result.group_id).trim() || groupId,
      closed: !!resp.result.closed,
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.browser_surface),
    },
  };
}

export function getPresentationBrowserSurfaceWebSocketUrl(groupId: string, slotId: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const base = `${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(groupId)}/presentation/browser_surface/ws?slot=${encodeURIComponent(slotId)}`;
  return withAuthToken(base);
}

export async function createGroup(title: string, topic: string = "") {
  clearGroupsReadRequest();
  return apiJson<{ group_id: string }>("/api/v1/groups", {
    method: "POST",
    body: JSON.stringify({ title, topic, by: "user" }),
  });
}

export async function createGroupFromTemplate(path: string, title: string, topic: string, file: File) {
  clearGroupsReadRequest();
  const form = new FormData();
  form.append("path", path);
  form.append("title", title);
  form.append("topic", topic || "");
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ group_id: string }>("/api/v1/groups/from_template", form);
}

export async function previewTemplate(file: File) {
  const form = new FormData();
  form.append("file", file);
  return apiForm<{ template: unknown }>("/api/v1/templates/preview", form);
}

export async function updateGroup(groupId: string, title: string, topic: string) {
  clearGroupsReadRequest();
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}`, {
    method: "PUT",
    body: JSON.stringify({ title: title.trim(), topic: topic.trim(), by: "user" }),
  });
}

export async function deleteGroup(groupId: string) {
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}?confirm=${encodeURIComponent(groupId)}&by=user`,
    { method: "DELETE" },
  );
}

export async function attachScope(groupId: string, path: string) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/attach`, {
    method: "POST",
    body: JSON.stringify({ path, by: "user" }),
  });
}

export async function startGroup(groupId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/start?by=user`, {
    method: "POST",
  });
}

export async function stopGroup(groupId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/stop?by=user`, {
    method: "POST",
  });
}

export async function setGroupState(groupId: string, state: "active" | "idle" | "paused") {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/state?state=${encodeURIComponent(state)}&by=user`,
    { method: "POST" },
  );
}

export async function exportGroupTemplate(groupId: string) {
  return apiJson<{ template: string; filename: string }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/template/export`,
  );
}

export async function previewGroupTemplate(groupId: string, file: File) {
  const form = new FormData();
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ template: unknown; diff: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/template/preview_upload`,
    form,
  );
}

export async function importGroupTemplateReplace(groupId: string, file: File) {
  const form = new FormData();
  form.append("confirm", groupId);
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ applied: boolean }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/template/import_replace`,
    form,
  );
}

export type GroupPromptKind = "preamble" | "help";

export type GroupPromptInfo = {
  kind: GroupPromptKind;
  source: "home" | "builtin";
  filename: string;
  path?: string | null;
  content: string;
};

export type GroupPromptsResponse = {
  preamble: GroupPromptInfo;
  help: GroupPromptInfo;
};

export type PetPeerContextResponse = {
  companion?: {
    name?: string | null;
    species?: string | null;
    identity?: string | null;
    temperament?: string | null;
    speech_style?: string | null;
    care_style?: string | null;
  };
  decisions?: Array<{
    id?: string;
    kind?: string;
    priority?: number;
    summary?: string | null;
    confidence?: "low" | "medium" | "high" | string | null;
    reasoning_brief?: string | null;
    agent?: string | null;
    fingerprint?: string | null;
    ephemeral?: boolean;
    source?: {
      event_id?: string | null;
      task_id?: string | null;
      actor_id?: string | null;
      actor_role?: string | null;
      error_reason?: string | null;
      suggestion_kind?: "mention" | "reply_required" | string | null;
    };
    action?: {
      type?: "draft_message" | "restart_actor" | "task_proposal" | string;
      group_id?: string | null;
      actor_id?: string | null;
      text?: string | null;
      to?: string[];
      reply_to?: string | null;
      operation?: "create" | "update" | "move" | "handoff" | "archive" | string | null;
      task_id?: string | null;
      title?: string | null;
      status?: string | null;
      assignee?: string | null;
    };
    updated_at?: string | null;
  }>;
  task_evidence?: Array<{
    kind?: string | null;
    priority?: number;
    hypothesis?: string | null;
    actor?: {
      id?: string | null;
      active_task_id?: string | null;
      focus?: string | null;
      next_action?: string | null;
      blockers?: string[] | null;
    };
    task?: {
      id?: string | null;
      title?: string | null;
      status?: string | null;
      assignee?: string | null;
      waiting_on?: string | null;
      blocked_by?: string[] | null;
      handoff_to?: string | null;
      updated_at?: string | null;
    };
    current_active_task?: {
      id?: string | null;
      title?: string | null;
      status?: string | null;
      assignee?: string | null;
      waiting_on?: string | null;
      blocked_by?: string[] | null;
      handoff_to?: string | null;
      updated_at?: string | null;
    };
    signals?: {
      task_stale_minutes?: number;
      same_workstream_hint?: boolean;
      blocker_count?: number;
    };
  }>;
  persona: string;
  help?: string;
  prompt?: string;
  snapshot: string;
  signals?: {
    reply_pressure?: {
      severity?: string;
      pending_count?: number;
      overdue_count?: number;
      oldest_pending_seconds?: number;
      baseline_median_reply_seconds?: number;
    };
    coordination_rhythm?: {
      severity?: string;
      foreman_id?: string;
      silence_seconds?: number;
      baseline_median_gap_seconds?: number;
    };
    task_pressure?: {
      severity?: string;
      score?: number;
      trend_score?: number;
      blocked_count?: number;
      waiting_user_count?: number;
      handoff_count?: number;
      planned_backlog_count?: number;
      recent_blocked_updates?: number;
      recent_waiting_user_updates?: number;
      recent_handoff_updates?: number;
      recent_task_create_ops?: number;
      recent_task_update_ops?: number;
      recent_task_move_ops?: number;
      recent_task_restore_ops?: number;
      recent_task_delete_ops?: number;
      recent_task_change_count?: number;
      recent_task_context_sync_events?: number;
      ledger_trend_score?: number;
    };
    proposal_ready?: {
      ready?: boolean;
      focus?: string;
      severity?: string;
      summary?: string;
      pending_reply_count?: number;
      overdue_reply_count?: number;
      waiting_user_count?: number;
      blocked_count?: number;
      handoff_count?: number;
      recent_task_change_count?: number;
      foreman_silence_seconds?: number;
    };
  };
  source: "help" | "default";
  help_prompt?: GroupPromptInfo;
};

export type PromptUpdateOptions = {
  editorMode?: "structured" | "raw";
  changedBlocks?: string[];
};

export async function fetchGroupPrompts(groupId: string) {
  const gid = String(groupId || "").trim();
  return reuseSharedReadRequest(
    groupPromptsKey(gid),
    () => apiJson<GroupPromptsResponse>(`/api/v1/groups/${encodeURIComponent(gid)}/prompts`),
  );
}

export async function fetchPetPeerContext(groupId: string, opts?: { fresh?: boolean; verbose?: boolean }) {
  const gid = String(groupId || "").trim();
  const fresh = !!opts?.fresh;
  const verbose = !!opts?.verbose;
  const params = new URLSearchParams();
  if (fresh) params.set("fresh", "1");
  if (verbose) params.set("verbose", "1");
  const suffix = params.toString();
  return reuseSharedReadRequest(
    petPeerContextRequestKey(gid, fresh, verbose),
    () =>
      apiJson<PetPeerContextResponse>(
        `/api/v1/groups/${encodeURIComponent(gid)}/pet-context${suffix ? `?${suffix}` : ""}`,
      ),
  );
}

export async function requestPetPeerReview(groupId: string) {
  const gid = String(groupId || "").trim();
  clearSharedReadRequest(petPeerContextRequestKey(gid, false, false));
  clearSharedReadRequest(petPeerContextRequestKey(gid, false, true));
  clearSharedReadRequest(petPeerContextRequestKey(gid, true, false));
  clearSharedReadRequest(petPeerContextRequestKey(gid, true, true));
  return apiJson<{ accepted?: boolean }>(`/api/v1/groups/${encodeURIComponent(gid)}/pet-context/review`, {
    method: "POST",
  });
}

export async function recordPetDecisionOutcome(
  groupId: string,
  payload: {
    fingerprint: string;
    outcome: "executed" | "dismissed";
    decisionId?: string;
    actionType?: string;
    cooldownMs?: number;
    sourceEventId?: string;
  },
) {
  const gid = String(groupId || "").trim();
  clearSharedReadRequest(petPeerContextRequestKey(gid, false, false));
  clearSharedReadRequest(petPeerContextRequestKey(gid, false, true));
  clearSharedReadRequest(petPeerContextRequestKey(gid, true, false));
  clearSharedReadRequest(petPeerContextRequestKey(gid, true, true));
  return apiJson<{ event?: unknown }>(`/api/v1/groups/${encodeURIComponent(gid)}/pet-decisions/outcome`, {
    method: "POST",
    body: JSON.stringify({
      fingerprint: String(payload.fingerprint || "").trim(),
      outcome: payload.outcome,
      decision_id: String(payload.decisionId || "").trim(),
      action_type: String(payload.actionType || "").trim(),
      cooldown_ms: Number(payload.cooldownMs || 0),
      source_event_id: String(payload.sourceEventId || "").trim(),
      by: "user",
    }),
  });
}

export async function updateGroupPrompt(
  groupId: string,
  kind: GroupPromptKind,
  content: string,
  opts?: PromptUpdateOptions,
) {
  clearSharedReadRequest(groupPromptsRequestKey(groupId));
  const body: Record<string, unknown> = { content, by: "user" };
  if (opts?.editorMode) body.editor_mode = opts.editorMode;
  if (Array.isArray(opts?.changedBlocks)) body.changed_blocks = opts.changedBlocks;
  return apiJson<GroupPromptInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/prompts/${kind}`, {
    method: "PUT",
    body: JSON.stringify(body),
  });
}

export async function resetGroupPrompt(groupId: string, kind: GroupPromptKind) {
  clearSharedReadRequest(groupPromptsRequestKey(groupId));
  return apiJson<GroupPromptInfo>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/prompts/${kind}?confirm=${encodeURIComponent(kind)}`,
    { method: "DELETE" },
  );
}
