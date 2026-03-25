// API service layer (centralized API calls).
import type {
  AgentState,
  CoordinationBrief,
  GroupMeta,
  GroupDoc,
  LedgerEvent,
  Actor,
  RuntimeInfo,
  GroupContext,
  GroupSettings,
  GroupAutomation,
  AutomationRuleSet,
  ActorProfile,
  ActorProfileUsage,
  Task,
  DirItem,
  DirSuggestion,
  IMConfig,
  IMStatus,
  IMPlatform,
  RemoteAccessState,
  WebAccessSession,
  WebBranding,
  GroupSpaceStatus,
  GroupSpaceRemoteSpace,
  GroupSpaceSource,
  GroupSpaceArtifact,
  GroupSpaceJob,
  GroupSpaceProviderCredentialStatus,
  GroupSpaceProviderAuthStatus,
  CapabilityOverviewItem,
  CapabilitySourceState,
  CapabilityBlockEntry,
  CapabilityStateResult,
  CapabilityImportRecord,
  GroupTasksSummary,
  TaskBoardEntry,
  TaskChecklistItem,
  ContextDetailLevel,
  GroupPresentation,
  MessageRef,
  PresentationCard,
  PresentationCardType,
  PresentationContent,
  PresentationRefSnapshot,
  PresentationSlot,
  PresentationTableData,
  PresentationWorkspaceItem,
  PresentationWorkspaceListing,
  PresentationBrowserSurfaceState,
} from "../types";
import { actorProfileIdentityKey } from "../utils/actorProfiles";

// ============ Access token auth ============

// Extract and cache token from URL for dev mode (Vite doesn't proxy /ui/ to backend)
let cachedToken: string | null = null;
const FORCE_LOGIN_KEY = "cccc_force_token_login";

// Global callback for 401 unauthorized responses
let _authRequiredHandler: (() => void) | null = null;

export function onAuthRequired(handler: () => void): void {
  _authRequiredHandler = handler;
}

function clearAllReadRequestCaches(): void {
  // Auth principal changed. Any shared/recent read keyed without principal
  // must be discarded so the next read is forced onto the new identity.
  globalReadEpoch += 1;
  sharedReadRequests.clear();
  recentReadResponses.clear();
  recentReadGenerations.clear();
}

export function setAuthToken(token: string): void {
  clearAllReadRequestCaches();
  cachedToken = token;
  try {
    sessionStorage.setItem("cccc_dev_token", token);
  } catch {
    void 0;
  }
}

export function clearAuthToken(): void {
  clearAllReadRequestCaches();
  cachedToken = "";
  try {
    sessionStorage.removeItem("cccc_dev_token");
  } catch {
    void 0;
  }
}

export function setForceTokenLogin(): void {
  try {
    sessionStorage.setItem(FORCE_LOGIN_KEY, "1");
  } catch {
    void 0;
  }
}

export function clearForceTokenLogin(): void {
  try {
    sessionStorage.removeItem(FORCE_LOGIN_KEY);
  } catch {
    void 0;
  }
}

export function shouldForceTokenLogin(): boolean {
  try {
    return sessionStorage.getItem(FORCE_LOGIN_KEY) === "1";
  } catch {
    return false;
  }
}

function getAuthToken(): string | null {
  if (cachedToken !== null) return cachedToken || null;

  // Check URL param first
  const urlParams = new URLSearchParams(window.location.search);
  const urlToken = urlParams.get("token");
  if (urlToken) {
    cachedToken = urlToken;
    try {
      sessionStorage.setItem("cccc_dev_token", urlToken);
    } catch {
      void 0;
    }
    return urlToken;
  }

  // Fallback to sessionStorage
  try {
    const stored = sessionStorage.getItem("cccc_dev_token");
    if (stored) {
      cachedToken = stored;
      return stored;
    }
  } catch {
    void 0;
  }

  cachedToken = "";
  return null;
}

function getAuthHeaders(): Record<string, string> {
  const token = getAuthToken();
  return token ? { Authorization: `Bearer ${token}` } : {};
}

// Build URL with token query param (for EventSource which doesn't support headers)
export function withAuthToken(url: string): string {
  const token = getAuthToken();
  if (!token) return url;
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}token=${encodeURIComponent(token)}`;
}

// ============ Base types & helpers ============

export type ApiResponse<T> =
  | { ok: true; result: T; error?: null }
  | { ok: false; result?: unknown; error: { code: string; message: string; details?: unknown } };

type SharedReadPromise = Promise<ApiResponse<unknown>>;
type RecentReadEntry = {
  response: ApiResponse<unknown>;
  expiresAt: number;
};

// Helper to create a typed error response.
function makeErrorResponse<T>(code: string, message: string): ApiResponse<T> {
  return { ok: false, error: { code, message } };
}

const sharedReadRequests = new Map<string, SharedReadPromise>();
const recentReadResponses = new Map<string, RecentReadEntry>();
const recentReadGenerations = new Map<string, number>();
const RECENT_BOOTSTRAP_READ_TTL_MS = 1000;
let globalReadEpoch = 0;

function reuseSharedReadRequest<T>(key: string, loader: () => Promise<ApiResponse<T>>): Promise<ApiResponse<T>> {
  const hit = sharedReadRequests.get(key);
  if (hit) return hit as Promise<ApiResponse<T>>;

  const task = loader().finally(() => {
    if (sharedReadRequests.get(key) === task) {
      sharedReadRequests.delete(key);
    }
  }) as SharedReadPromise;
  sharedReadRequests.set(key, task);
  return task as Promise<ApiResponse<T>>;
}

function reuseRecentReadRequest<T>(
  key: string,
  ttlMs: number,
  loader: () => Promise<ApiResponse<T>>
): Promise<ApiResponse<T>> {
  const cached = recentReadResponses.get(key);
  if (cached && cached.expiresAt > Date.now()) {
    return Promise.resolve(cached.response as ApiResponse<T>);
  }
  const requestEpoch = globalReadEpoch;
  const requestGeneration = recentReadGenerations.get(key) || 0;
  return reuseSharedReadRequest(key, async () => {
    const response = await loader();
    const generationStillCurrent =
      globalReadEpoch === requestEpoch && (recentReadGenerations.get(key) || 0) === requestGeneration;
    if (response.ok && generationStillCurrent) {
      recentReadResponses.set(key, {
        response: response as ApiResponse<unknown>,
        expiresAt: Date.now() + ttlMs,
      });
    } else {
      recentReadResponses.delete(key);
    }
    return response;
  });
}

function clearSharedReadRequest(key: string): void {
  sharedReadRequests.delete(key);
}

function clearRecentReadRequest(key: string): void {
  recentReadGenerations.set(key, (recentReadGenerations.get(key) || 0) + 1);
  recentReadResponses.delete(key);
  clearSharedReadRequest(key);
}

function actorsReadOnlyRequestKey(groupId: string): string {
  return `actors:${String(groupId || "").trim()}:read-only`;
}

function groupsRequestKey(): string {
  return "groups:list";
}

function groupPromptsRequestKey(groupId: string): string {
  return `group-prompts:${String(groupId || "").trim()}`;
}

function petPeerContextRequestKey(groupId: string, fresh: boolean): string {
  return `pet-peer-context:${String(groupId || "").trim()}:${fresh ? "fresh" : "default"}`;
}

function pingRequestKey(includeHome: boolean): string {
  return includeHome ? "ping:include-home" : "ping:default";
}

function webAccessSessionRequestKey(): string {
  return "web-access-session";
}

function contextRequestKey(groupId: string, detail: ContextDetailLevel): string {
  return `context:${String(groupId || "").trim()}:${detail}`;
}

function clearGroupsReadRequest(): void {
  clearRecentReadRequest(groupsRequestKey());
}

export function invalidateGroupsRead(): void {
  clearGroupsReadRequest();
}

function clearPingReadRequest(includeHome?: boolean): void {
  if (typeof includeHome === "boolean") {
    clearRecentReadRequest(pingRequestKey(includeHome));
    return;
  }
  clearRecentReadRequest(pingRequestKey(false));
  clearRecentReadRequest(pingRequestKey(true));
}

function clearWebAccessSessionReadRequest(): void {
  clearRecentReadRequest(webAccessSessionRequestKey());
}

function clearActorsReadOnlyRequest(groupId: string): void {
  clearSharedReadRequest(actorsReadOnlyRequestKey(groupId));
}

function clearContextRequest(groupId: string, detail?: ContextDetailLevel): void {
  if (detail) {
    clearSharedReadRequest(contextRequestKey(groupId, detail));
    return;
  }
  clearSharedReadRequest(contextRequestKey(groupId, "summary"));
  clearSharedReadRequest(contextRequestKey(groupId, "full"));
}

export function invalidateContextRead(groupId: string): void {
  clearContextRequest(groupId);
}

export type FetchContextOptions = {
  fresh?: boolean;
  detail?: ContextDetailLevel;
};

type UnknownRecord = Record<string, unknown>;

function asRecord(value: unknown): UnknownRecord | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as UnknownRecord) : null;
}

function asString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

function asOptionalString(value: unknown): string | null {
  const text = asString(value).trim();
  return text ? text : null;
}

function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => asString(item).trim()).filter(Boolean);
}

function normalizeChecklistItem(value: unknown, index: number): TaskChecklistItem | null {
  const record = asRecord(value);
  if (!record) return null;
  const text = asString(record.text).trim();
  if (!text) return null;
  const id = asString(record.id).trim() || `item-${index + 1}`;
  const status = asString(record.status).trim() || "pending";
  return { id, text, status };
}

function normalizeTask(value: unknown): Task | null {
  const record = asRecord(value);
  if (!record) return null;
  const id = asString(record.id).trim();
  if (!id) return null;

  const title = asString(record.title).trim() || id;
  const outcome = asString(record.outcome);
  const checklist = Array.isArray(record.checklist)
    ? record.checklist
        .map((item, index) => normalizeChecklistItem(item, index))
        .filter((item): item is TaskChecklistItem => !!item)
    : [];

  return {
    id,
    title,
    outcome,
    status: asString(record.status).trim() || undefined,
    assignee: asOptionalString(record.assignee),
    priority: asOptionalString(record.priority),
    parent_id: asOptionalString(record.parent_id),
    blocked_by: asStringArray(record.blocked_by),
    waiting_on: asString(record.waiting_on).trim() || undefined,
    handoff_to: asOptionalString(record.handoff_to),
    notes: asString(record.notes),
    checklist,
    created_at: asOptionalString(record.created_at),
    updated_at: asOptionalString(record.updated_at),
    archived_from: asOptionalString(record.archived_from),
    progress: typeof record.progress === "number" ? record.progress : checklist.length > 0
      ? checklist.filter((item) => item.status === "done").length / checklist.length
      : undefined,
  };
}

function normalizeAgentState(value: unknown): AgentState | null {
  const record = asRecord(value);
  if (!record) return null;
  const id = asString(record.id).trim();
  if (!id) return null;

  const hotRecord = asRecord(record.hot);
  const warmRecord = asRecord(record.warm);

  return {
    id,
    hot: {
      active_task_id: asOptionalString(hotRecord?.active_task_id),
      focus: asOptionalString(hotRecord?.focus),
      next_action: asOptionalString(hotRecord?.next_action),
      blockers: asStringArray(hotRecord?.blockers),
    },
    warm: {
      what_changed: asOptionalString(warmRecord?.what_changed),
      open_loops: asStringArray(warmRecord?.open_loops),
      commitments: asStringArray(warmRecord?.commitments),
      environment_summary: asOptionalString(warmRecord?.environment_summary),
      user_model: asOptionalString(warmRecord?.user_model),
      persona_notes: asOptionalString(warmRecord?.persona_notes),
      resume_hint: asOptionalString(warmRecord?.resume_hint),
    },
    updated_at: asOptionalString(record.updated_at),
  };
}

function normalizeTaskSummary(value: unknown, tasks: Task[]): GroupTasksSummary {
  const record = asRecord(value);
  if (record) {
    return {
      total: Number(record.total || 0),
      planned: Number(record.planned || 0),
      active: Number(record.active || 0),
      done: Number(record.done || 0),
      archived: Number(record.archived || 0) || undefined,
      root_count: Number(record.root_count || 0) || undefined,
    };
  }

  const summary: GroupTasksSummary = { total: tasks.length, planned: 0, active: 0, done: 0, archived: 0 };
  for (const task of tasks) {
    const status = asString(task.status || "planned").toLowerCase();
    if (status === "active") summary.active += 1;
    else if (status === "done") summary.done += 1;
    else if (status === "archived") summary.archived = Number(summary.archived || 0) + 1;
    else summary.planned += 1;
  }
  return summary;
}

function normalizeBoardEntries(value: unknown): TaskBoardEntry[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const items: Array<string | Task | null> = value.map((item) => {
    if (typeof item === "string") {
      const id = item.trim();
      return id ? id : null;
    }
    return normalizeTask(item);
  });
  return items.filter((item): item is string | Task => item !== null);
}

function normalizeContext(raw: unknown): GroupContext {
  const record = asRecord(raw) ?? {};
  const coordination = asRecord(record.coordination) ?? {};
  const tasks = Array.isArray(coordination.tasks)
    ? coordination.tasks.map((item) => normalizeTask(item)).filter((item): item is Task => !!item)
    : [];
  const agentStates = Array.isArray(record.agent_states)
    ? record.agent_states.map((item) => normalizeAgentState(item)).filter((item): item is AgentState => !!item)
    : [];
  const briefRecord = asRecord(coordination.brief);
  const summary = normalizeTaskSummary(record.tasks_summary, tasks);
  const boardRecord = asRecord(record.board);
  const attentionRecord = asRecord(record.attention);
  const metaRecord = asRecord(record.meta);

  return {
    version: asString(record.version).trim() || undefined,
    coordination: {
      brief: briefRecord
        ? {
            objective: asString(briefRecord.objective),
            current_focus: asString(briefRecord.current_focus),
            constraints: asStringArray(briefRecord.constraints),
            project_brief: asString(briefRecord.project_brief),
            project_brief_stale: !!briefRecord.project_brief_stale,
            updated_by: asOptionalString(briefRecord.updated_by),
            updated_at: asOptionalString(briefRecord.updated_at),
          }
        : null,
      tasks,
      recent_decisions: Array.isArray(coordination.recent_decisions)
        ? coordination.recent_decisions
            .map((item) => asRecord(item))
            .filter((item): item is UnknownRecord => !!item)
            .map((item) => ({
              at: asOptionalString(item.at),
              by: asOptionalString(item.by),
              summary: asString(item.summary),
              task_id: asOptionalString(item.task_id),
            }))
        : [],
      recent_handoffs: Array.isArray(coordination.recent_handoffs)
        ? coordination.recent_handoffs
            .map((item) => asRecord(item))
            .filter((item): item is UnknownRecord => !!item)
            .map((item) => ({
              at: asOptionalString(item.at),
              by: asOptionalString(item.by),
              summary: asString(item.summary),
              task_id: asOptionalString(item.task_id),
            }))
        : [],
    },
    agent_states: agentStates,
    attention: attentionRecord
      ? {
          blocked: Array.isArray(attentionRecord.blocked) ? normalizeBoardEntries(attentionRecord.blocked) || [] : typeof attentionRecord.blocked === "number" ? attentionRecord.blocked : undefined,
          waiting_user: Array.isArray(attentionRecord.waiting_user) ? normalizeBoardEntries(attentionRecord.waiting_user) || [] : typeof attentionRecord.waiting_user === "number" ? attentionRecord.waiting_user : undefined,
          pending_handoffs: Array.isArray(attentionRecord.pending_handoffs) ? normalizeBoardEntries(attentionRecord.pending_handoffs) || [] : typeof attentionRecord.pending_handoffs === "number" ? attentionRecord.pending_handoffs : undefined,
        }
      : null,
    board: boardRecord
      ? {
          planned: normalizeBoardEntries(boardRecord.planned),
          active: normalizeBoardEntries(boardRecord.active),
          done: normalizeBoardEntries(boardRecord.done),
          archived: normalizeBoardEntries(boardRecord.archived),
        }
      : null,
    tasks_summary: summary,
    meta: metaRecord ? { ...metaRecord } : {},
  };
}

function normalizePresentationTable(value: unknown): PresentationTableData | null {
  const record = asRecord(value);
  if (!record) return null;
  const columns = Array.isArray(record.columns) ? record.columns.map((item) => asString(item)) : [];
  const rows = Array.isArray(record.rows)
    ? record.rows.map((row) =>
        Array.isArray(row) ? row.map((cell) => asString(cell)) : []
      )
    : [];
  return { columns, rows };
}

function normalizePresentationContent(value: unknown): PresentationContent {
  const record = asRecord(value) ?? {};
  const rawMode = asString(record.mode).trim();
  return {
    mode: rawMode === "reference" || rawMode === "workspace_link" ? rawMode : "inline",
    markdown: asOptionalString(record.markdown),
    table: normalizePresentationTable(record.table),
    url: asOptionalString(record.url),
    blob_rel_path: asOptionalString(record.blob_rel_path),
    workspace_rel_path: asOptionalString(record.workspace_rel_path),
    mime_type: asOptionalString(record.mime_type),
    file_name: asOptionalString(record.file_name),
  };
}

function normalizePresentationCard(value: unknown): PresentationCard | null {
  const record = asRecord(value);
  if (!record) return null;
  const slotId = asString(record.slot_id).trim();
  const title = asString(record.title).trim();
  if (!slotId || !title) return null;

  const rawCardType = asString(record.card_type).trim();
  const cardType: PresentationCardType =
    rawCardType === "markdown" ||
    rawCardType === "table" ||
    rawCardType === "image" ||
    rawCardType === "pdf" ||
    rawCardType === "web_preview"
      ? rawCardType
      : "file";

  return {
    slot_id: slotId,
    title,
    card_type: cardType,
    published_by: asString(record.published_by).trim() || "user",
    published_at: asString(record.published_at).trim(),
    source_label: asString(record.source_label).trim(),
    source_ref: asString(record.source_ref).trim(),
    summary: asString(record.summary).trim(),
    content: normalizePresentationContent(record.content),
  };
}

function normalizePresentationSlot(value: unknown, fallbackIndex: number): PresentationSlot {
  const record = asRecord(value) ?? {};
  const index = Number(record.index || fallbackIndex);
  const normalizedIndex = Number.isFinite(index) && index > 0 ? index : fallbackIndex;
  const slotId = asString(record.slot_id).trim() || `slot-${normalizedIndex}`;
  return {
    slot_id: slotId,
    index: normalizedIndex,
    card: normalizePresentationCard(record.card),
  };
}

function normalizePresentation(raw: unknown): GroupPresentation {
  const record = asRecord(raw) ?? {};
  const rawSlots = Array.isArray(record.slots) ? record.slots : [];
  const slotsById = new Map<string, PresentationSlot>();
  rawSlots.forEach((slot, index) => {
    const normalized = normalizePresentationSlot(slot, index + 1);
    slotsById.set(normalized.slot_id, normalized);
  });

  const orderedSlots = Array.from({ length: 4 }, (_, index) => {
    const slotId = `slot-${index + 1}`;
    const existing = slotsById.get(slotId);
    return existing || { slot_id: slotId, index: index + 1, card: null };
  });

  const highlightSlotId = asString(record.highlight_slot_id).trim();
  return {
    v: Number(record.v || 1) || 1,
    updated_at: asString(record.updated_at).trim(),
    highlight_slot_id: highlightSlotId,
    slots: orderedSlots,
  };
}

function normalizePresentationBrowserSurfaceState(value: unknown): PresentationBrowserSurfaceState {
  const record = asRecord(value) ?? {};
  const errorRecord = asRecord(record.error);
  return {
    active: !!record.active,
    state: asString(record.state).trim() || "idle",
    message: asOptionalString(record.message),
    error: errorRecord
      ? {
          code: asOptionalString(errorRecord.code),
          message: asOptionalString(errorRecord.message),
        }
      : null,
    strategy: asOptionalString(record.strategy),
    url: asOptionalString(record.url),
    width: Number.isFinite(Number(record.width)) ? Number(record.width) : 0,
    height: Number.isFinite(Number(record.height)) ? Number(record.height) : 0,
    started_at: asOptionalString(record.started_at),
    updated_at: asOptionalString(record.updated_at),
    last_frame_seq: Number.isFinite(Number(record.last_frame_seq)) ? Number(record.last_frame_seq) : 0,
    last_frame_at: asOptionalString(record.last_frame_at),
    controller_attached: !!record.controller_attached,
  };
}

export async function apiJson<T>(path: string, init?: RequestInit): Promise<ApiResponse<T>> {
  let resp: Response;
  try {
    resp = await fetch(path, {
      ...init,
      headers: {
        "content-type": "application/json",
        ...getAuthHeaders(),
        ...(init?.headers || {}),
      },
    });
  } catch (e) {
    // Network error
    return makeErrorResponse("NETWORK_ERROR", e instanceof Error ? e.message : "Network request failed");
  }
  if (resp.status === 401) {
    _authRequiredHandler?.();
  }

  const text = await resp.text();
  if (!text) {
    // Empty body but 2xx
    if (resp.ok) {
      return { ok: true, result: {} as T };
    }
    return makeErrorResponse("EMPTY_RESPONSE", `Server returned ${resp.status} with empty body`);
  }

  try {
    const data = JSON.parse(text);
    if (!data.ok && data.error?.code === "unauthorized") {
      _authRequiredHandler?.();
    }
    return data as ApiResponse<T>;
  } catch {
    // JSON parse error
    return makeErrorResponse("PARSE_ERROR", `Invalid JSON response: ${text.slice(0, 100)}`);
  }
}

export async function apiForm<T>(path: string, form: FormData, init?: RequestInit): Promise<ApiResponse<T>> {
  let resp: Response;
  try {
    resp = await fetch(path, {
      ...(init || {}),
      method: init?.method || "POST",
      body: form,
      headers: {
        ...getAuthHeaders(),
        ...(init?.headers || {}),
      },
    });
  } catch (e) {
    return makeErrorResponse("NETWORK_ERROR", e instanceof Error ? e.message : "Network request failed");
  }
  if (resp.status === 401) {
    _authRequiredHandler?.();
  }

  const text = await resp.text();
  if (!text) {
    if (resp.ok) {
      return { ok: true, result: {} as T };
    }
    return makeErrorResponse("EMPTY_RESPONSE", `Server returned ${resp.status} with empty body`);
  }

  try {
    const data = JSON.parse(text);
    if (!data.ok && data.error?.code === "unauthorized") {
      _authRequiredHandler?.();
    }
    return data as ApiResponse<T>;
  } catch {
    return makeErrorResponse("PARSE_ERROR", `Invalid JSON response: ${text.slice(0, 100)}`);
  }
}

// ============ Groups ============

export async function fetchGroups() {
  return reuseRecentReadRequest(
    groupsRequestKey(),
    RECENT_BOOTSTRAP_READ_TTL_MS,
    () => apiJson<{ groups: GroupMeta[] }>("/api/v1/groups")
  );
}

export async function fetchPing(options?: { includeHome?: boolean }) {
  const includeHome = Boolean(options?.includeHome);
  const suffix = includeHome ? "?include_home=1" : "";
  return reuseRecentReadRequest(
    pingRequestKey(includeHome),
    RECENT_BOOTSTRAP_READ_TTL_MS,
    () => apiJson<{ home?: string; daemon: unknown; version: string; web?: { mode?: string; read_only?: boolean } }>(
      `/api/v1/ping${suffix}`
    )
  );
}

export async function fetchGroup(groupId: string) {
  return apiJson<{ group: GroupDoc }>(`/api/v1/groups/${encodeURIComponent(groupId)}`);
}

export async function fetchPresentation(groupId: string): Promise<ApiResponse<{ group_id: string; presentation: GroupPresentation }>> {
  const resp = await apiJson<{ group_id?: string; presentation?: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation`
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
    cleared_slots: Array.isArray(result.cleared_slots) ? result.cleared_slots.map((slot) => asString(slot).trim()).filter(Boolean) : undefined,
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
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/slots/${encodeURIComponent(slotId)}/asset`
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
    `/api/v1/groups/${encodeURIComponent(groupId)}/presentation/browser_surface/session?slot=${encodeURIComponent(slotId)}`
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
    }
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
    }
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

export async function createGroupFromTemplate(
  path: string,
  title: string,
  topic: string,
  file: File
) {
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
    { method: "DELETE" }
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
    { method: "POST" }
  );
}

// ============ Group Templates (Replace) ============

export async function exportGroupTemplate(groupId: string) {
  return apiJson<{ template: string; filename: string }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/template/export`
  );
}

export async function previewGroupTemplate(groupId: string, file: File) {
  const form = new FormData();
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ template: unknown; diff: unknown }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/template/preview_upload`,
    form
  );
}

export async function importGroupTemplateReplace(groupId: string, file: File) {
  const form = new FormData();
  form.append("confirm", groupId);
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ applied: boolean }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/template/import_replace`,
    form
  );
}

// ============ Ledger ============

export async function fetchLedgerTail(groupId: string, lines = 120) {
  const params = new URLSearchParams({
    kind: "chat",
    limit: String(lines),
    with_read_status: "true",
    with_ack_status: "true",
    with_obligation_status: "true",
  });
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`
  );
}

export async function fetchOlderMessages(
  groupId: string,
  beforeEventId: string,
  limit = 50
) {
  const params = new URLSearchParams({
    kind: "chat",
    before: beforeEventId,
    limit: String(limit),
    with_read_status: "true",
    with_ack_status: "true",
    with_obligation_status: "true",
  });
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`
  );
}

export async function fetchMessageWindow(
  groupId: string,
  centerEventId: string,
  opts?: { before?: number; after?: number }
) {
  const params = new URLSearchParams({
    kind: "chat",
    center: centerEventId,
    before: String(opts?.before ?? 30),
    after: String(opts?.after ?? 30),
    with_read_status: "true",
    with_ack_status: "true",
    with_obligation_status: "true",
  });
  return apiJson<{
    center_id: string;
    center_index: number;
    events: LedgerEvent[];
    has_more_before: boolean;
    has_more_after: boolean;
    count: number;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/ledger/window?${params.toString()}`);
}

export async function searchChatMessages(
  groupId: string,
  q: string,
  opts?: { limit?: number; before?: string; after?: string }
) {
  const params = new URLSearchParams({
    kind: "chat",
    q: q || "",
    limit: String(opts?.limit ?? 50),
    with_read_status: "true",
    with_ack_status: "true",
    with_obligation_status: "true",
  });
  if (opts?.before) params.set("before", opts.before);
  if (opts?.after) params.set("after", opts.after);
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`
  );
}

// ============ Actors ============

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
  persona: string;
  help: string;
  prompt: string;
  snapshot: string;
  source: "help" | "default";
  help_prompt: GroupPromptInfo;
};

export type PromptUpdateOptions = {
  editorMode?: "structured" | "raw";
  changedBlocks?: string[];
};

export async function fetchGroupPrompts(groupId: string) {
  const gid = String(groupId || "").trim();
  return reuseSharedReadRequest(
    groupPromptsRequestKey(gid),
    () => apiJson<GroupPromptsResponse>(`/api/v1/groups/${encodeURIComponent(gid)}/prompts`)
  );
}

export async function fetchPetPeerContext(groupId: string, opts?: { fresh?: boolean }) {
  const gid = String(groupId || "").trim();
  const fresh = !!opts?.fresh;
  const params = new URLSearchParams();
  if (fresh) params.set("fresh", "1");
  const suffix = params.toString();
  return reuseSharedReadRequest(
    petPeerContextRequestKey(gid, fresh),
    () =>
      apiJson<PetPeerContextResponse>(
        `/api/v1/groups/${encodeURIComponent(gid)}/pet-context${suffix ? `?${suffix}` : ""}`
      )
  );
}

export async function updateGroupPrompt(
  groupId: string,
  kind: GroupPromptKind,
  content: string,
  opts?: PromptUpdateOptions
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
  return apiJson<GroupPromptInfo>(`/api/v1/groups/${encodeURIComponent(groupId)}/prompts/${kind}?confirm=${encodeURIComponent(kind)}`, {
    method: "DELETE",
  });
}

// ============ Actors ============

export async function fetchActors(groupId: string, includeUnread = false) {
  const gid = String(groupId || "").trim();
  const url = includeUnread
    ? `/api/v1/groups/${encodeURIComponent(gid)}/actors?include_unread=true`
    : `/api/v1/groups/${encodeURIComponent(gid)}/actors`;
  if (includeUnread) {
    return apiJson<{ actors: Actor[] }>(url);
  }
  return reuseSharedReadRequest(
    actorsReadOnlyRequestKey(gid),
    () => apiJson<{ actors: Actor[] }>(url)
  );
}

export async function addActor(
  groupId: string,
  actorId: string,
  role: "peer" | "foreman",
  runtime: string,
  command: string,
  envPrivate?: Record<string, string>,
  options?: {
    profileId?: string;
    profileScope?: ProfileScope;
    profileOwner?: string;
    title?: string;
    capabilityAutoload?: string[];
  }
) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors`, {
    method: "POST",
    body: JSON.stringify({
      actor_id: actorId,
      role,
      runner: "pty",
      runtime,
      command,
      env: {},
      env_private: envPrivate && Object.keys(envPrivate).length ? envPrivate : undefined,
      profile_id: options?.profileId || undefined,
      profile_scope: options?.profileScope || undefined,
      profile_owner: options?.profileOwner || undefined,
      capability_autoload: Array.isArray(options?.capabilityAutoload)
        ? options?.capabilityAutoload
        : [],
      title: options?.title || "",
      default_scope_key: "",
      by: "user",
    }),
  });
}

export async function updateActor(
  groupId: string,
  actorId: string,
  runtime?: string,
  command?: string,
  title?: string,
  opts?: {
    profileId?: string;
    profileScope?: ProfileScope;
    profileOwner?: string;
    profileAction?: "convert_to_custom";
    enabled?: boolean;
    capabilityAutoload?: string[];
  }
) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  const body: Record<string, unknown> = { by: "user" };
  if (runtime !== undefined && runtime !== "") body.runtime = runtime;
  if (command !== undefined) body.command = command.trim();
  if (title !== undefined) body.title = title.trim();
  if (opts?.profileId !== undefined) body.profile_id = String(opts.profileId || "");
  if (opts?.profileScope !== undefined) body.profile_scope = String(opts.profileScope || "");
  if (opts?.profileOwner !== undefined) body.profile_owner = String(opts.profileOwner || "");
  if (opts?.profileAction) body.profile_action = opts.profileAction;
  if (typeof opts?.enabled === "boolean") body.enabled = opts.enabled;
  if (Array.isArray(opts?.capabilityAutoload)) body.capability_autoload = opts.capabilityAutoload;
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
    {
      method: "POST",
      body: JSON.stringify(body),
    }
  );
}

export async function attachActorProfile(groupId: string, actorId: string, profileId: string, opts?: ProfileLookupOptions) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson<{ actor: Actor }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        profile_id: profileId,
        profile_scope: opts?.scope || undefined,
        profile_owner: opts?.ownerId || undefined,
      }),
    }
  );
}

export async function convertActorToCustom(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson<{ actor: Actor }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", profile_action: "convert_to_custom" }),
    }
  );
}

export async function removeActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}?by=user`,
    { method: "DELETE" }
  );
}

export async function startActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/start`,
    { method: "POST" }
  );
}

export async function stopActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/stop`,
    { method: "POST" }
  );
}

export async function restartActor(groupId: string, actorId: string) {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/restart?by=user`,
    { method: "POST" }
  );
}

export async function fetchActorPrivateEnvKeys(groupId: string, actorId: string) {
  return apiJson<{ group_id: string; actor_id: string; keys: string[]; masked_values?: Record<string, string> }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/env_private?by=user`
  );
}

export async function updateActorPrivateEnv(
  groupId: string,
  actorId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean
) {
  return apiJson<{ group_id: string; actor_id: string; keys: string[] }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/env_private`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", set: setVars, unset: unsetKeys, clear }),
    }
  );
}

export type ProfileView = "global" | "my" | "all";
export type ProfileScope = "global" | "user";

type ProfileLookupOptions = {
  scope?: ProfileScope;
  ownerId?: string;
};

type ProfileDeleteOptions = ProfileLookupOptions & {
  forceDetach?: boolean;
};

function buildProfileQuery(opts?: ProfileLookupOptions): string {
  const params = new URLSearchParams();
  if (opts?.scope) params.set("scope", String(opts.scope));
  if (opts?.ownerId) params.set("owner_id", String(opts.ownerId));
  const query = params.toString();
  return query ? `?${query}` : "";
}

function buildProfileDeleteQuery(opts?: ProfileDeleteOptions): string {
  const params = new URLSearchParams();
  params.set("by", "user");
  if (opts?.scope) params.set("scope", String(opts.scope));
  if (opts?.ownerId) params.set("owner_id", String(opts.ownerId));
  if (opts?.forceDetach) params.set("force_detach", "true");
  return params.toString();
}

export async function listProfiles(view: ProfileView = "global") {
  return apiJson<{ profiles: ActorProfile[] }>(`/api/v1/profiles?view=${encodeURIComponent(view)}`);
}

export async function getProfile(profileId: string, opts?: ProfileLookupOptions) {
  return apiJson<{ profile: ActorProfile; usage: ActorProfileUsage[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}${buildProfileQuery(opts)}`
  );
}

export async function saveProfile(profile: Record<string, unknown>, expectedRevision?: number) {
  const profileId = String(profile.id || "").trim();
  if (!profileId) {
    const body: Record<string, unknown> = { by: "user", profile };
    if (typeof expectedRevision === "number") {
      body.expected_revision = Math.trunc(expectedRevision);
    }
    return apiJson<{ profile: ActorProfile }>(`/api/v1/actor_profiles`, {
      method: "POST",
      body: JSON.stringify(body),
    });
  }

  const body: Record<string, unknown> = { ...profile, by: "user" };
  if (typeof expectedRevision === "number") {
    body.expected_revision = Math.trunc(expectedRevision);
  }
  return apiJson<{ profile: ActorProfile }>(`/api/v1/profiles/${encodeURIComponent(profileId)}`, {
    method: "PUT",
    body: JSON.stringify(body),
  });
}

export async function deleteProfile(profileId: string, opts?: ProfileDeleteOptions) {
  return apiJson<{ deleted: boolean; profile_id: string; detached_count?: number; detached?: ActorProfileUsage[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}?${buildProfileDeleteQuery(opts)}`,
    { method: "DELETE" }
  );
}

export async function listActorProfiles(): Promise<ApiResponse<{ profiles: ActorProfile[] }>> {
  const [globalRes, myRes] = await Promise.all([
    listProfiles("global"),
    listProfiles("my"),
  ]);
  if (!globalRes.ok) return globalRes;
  if (!myRes.ok) return myRes;
  const seen = new Set<string>();
  const profiles: ActorProfile[] = [];
  for (const p of [...globalRes.result.profiles, ...myRes.result.profiles]) {
    const key = actorProfileIdentityKey(p);
    if (!seen.has(key)) {
      seen.add(key);
      profiles.push(p);
    }
  }
  return { ok: true, result: { profiles } };
}

export async function getActorProfile(profileId: string) {
  return getProfile(profileId);
}

export async function upsertActorProfile(profile: Record<string, unknown>, expectedRevision?: number) {
  return saveProfile({ ...profile, scope: "global", owner_id: "" }, expectedRevision);
}

export async function deleteActorProfile(profileId: string, opts?: { forceDetach?: boolean }) {
  return deleteProfile(profileId, { scope: "global", forceDetach: opts?.forceDetach });
}

export async function fetchActorProfilePrivateEnvKeys(profileId: string, opts?: ProfileLookupOptions) {
  return fetchProfilePrivateEnvKeys(profileId, opts ?? { scope: "global" });
}

export async function fetchProfilePrivateEnvKeys(profileId: string, opts?: ProfileLookupOptions) {
  return apiJson<{ profile_id: string; keys: string[]; masked_values?: Record<string, string> }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}/env_private${buildProfileQuery(opts)}${buildProfileQuery(opts) ? "&" : "?"}by=user`
  );
}

export async function updateActorProfilePrivateEnv(
  profileId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean
) {
  return updateProfilePrivateEnv(profileId, setVars, unsetKeys, clear, { scope: "global" });
}

export async function updateProfilePrivateEnv(
  profileId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean,
  opts?: ProfileLookupOptions
) {
  return apiJson<{ profile_id: string; keys: string[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}/env_private`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        scope: opts?.scope,
        owner_id: opts?.ownerId,
        set: setVars,
        unset: unsetKeys,
        clear,
      }),
    }
  );
}

export async function copyActorPrivateEnvToProfile(
  profileId: string,
  groupId: string,
  actorId: string,
  opts?: ProfileLookupOptions
) {
  return apiJson<{ profile_id: string; group_id: string; actor_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/copy_actor_secrets`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        scope: opts?.scope,
        owner_id: opts?.ownerId,
        group_id: groupId,
        actor_id: actorId,
      }),
    }
  );
}

export async function copyActorProfilePrivateEnvFromProfile(
  profileId: string,
  sourceProfileId: string,
  opts?: ProfileLookupOptions & {
    sourceScope?: ProfileScope;
    sourceOwnerId?: string;
  }
) {
  return copyProfilePrivateEnvFromProfile(profileId, sourceProfileId, {
    scope: "global",
    ...opts,
  });
}

export async function copyProfilePrivateEnvFromProfile(
  profileId: string,
  sourceProfileId: string,
  opts?: ProfileLookupOptions & {
    sourceScope?: ProfileScope;
    sourceOwnerId?: string;
  }
) {
  return apiJson<{ profile_id: string; source_profile_id: string; keys: string[] }>(
    `/api/v1/profiles/${encodeURIComponent(profileId)}/copy_profile_secrets`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        scope: opts?.scope,
        owner_id: opts?.ownerId,
        source_profile_id: sourceProfileId,
        source_scope: opts?.sourceScope ?? opts?.scope,
        source_owner_id: opts?.sourceOwnerId ?? opts?.ownerId,
      }),
    }
  );
}

// ============ Context & Settings ============

export async function fetchContext(groupId: string, opts?: FetchContextOptions) {
  const gid = String(groupId || "").trim();
  const detail: ContextDetailLevel = opts?.detail === "full" ? "full" : "summary";
  const params = new URLSearchParams();
  if (detail !== "summary") {
    params.set("detail", detail);
  }
  if (opts?.fresh) {
    clearContextRequest(gid);
    params.set("fresh", "1");
    params.set("_", String(Date.now()));
    return reuseSharedReadRequest(contextRequestKey(gid, detail), async () => {
      const resp = await apiJson<unknown>(
        `/api/v1/groups/${encodeURIComponent(gid)}/context?${params.toString()}`
      );
      if (!resp.ok) return resp as ApiResponse<GroupContext>;
      return { ok: true, result: normalizeContext(resp.result) } as ApiResponse<GroupContext>;
    });
  }
  return reuseSharedReadRequest(contextRequestKey(gid, detail), async () => {
    const suffix = params.toString();
    const resp = await apiJson<unknown>(
      `/api/v1/groups/${encodeURIComponent(gid)}/context${suffix ? `?${suffix}` : ""}`
    );
    if (!resp.ok) return resp as ApiResponse<GroupContext>;
    return { ok: true, result: normalizeContext(resp.result) } as ApiResponse<GroupContext>;
  });
}

export async function fetchTasks(groupId: string) {
  const resp = await apiJson<{ tasks?: unknown[] }>(`/api/v1/groups/${encodeURIComponent(groupId)}/tasks`);
  if (!resp.ok) return resp as ApiResponse<{ tasks: Task[] }>;
  const tasks = Array.isArray(resp.result?.tasks)
    ? resp.result.tasks.map((item) => normalizeTask(item)).filter((item): item is Task => !!item)
    : [];
  return { ok: true, result: { tasks } } as ApiResponse<{ tasks: Task[] }>;
}

export async function contextSync(
  groupId: string,
  ops: Array<Record<string, unknown>>,
  dryRun: boolean = false
) {
  clearContextRequest(groupId);
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/context`, {
    method: "POST",
    body: JSON.stringify({ ops, by: "user", dry_run: dryRun }),
  });
}

export async function updateCoordinationBrief(groupId: string, brief: CoordinationBrief) {
  const op: Record<string, unknown> = { op: "coordination.brief.update" };
  if (brief.objective !== undefined) op.objective = String(brief.objective || "");
  if (brief.current_focus !== undefined) op.current_focus = String(brief.current_focus || "");
  if (brief.constraints !== undefined) op.constraints = Array.isArray(brief.constraints) ? brief.constraints : [];
  if (brief.project_brief !== undefined) op.project_brief = String(brief.project_brief || "");
  if (brief.project_brief_stale !== undefined) op.project_brief_stale = !!brief.project_brief_stale;
  return contextSync(groupId, [op]);
}

export async function addCoordinationNote(
  groupId: string,
  kind: "decision" | "handoff",
  summary: string,
  taskId?: string | null
) {
  const op: Record<string, unknown> = { op: "coordination.note.add", kind, summary: String(summary || "") };
  if (taskId) op.task_id = String(taskId || "");
  return contextSync(groupId, [op]);
}

export async function updateCoordinationTask(groupId: string, task: Task) {
  const updateOp: Record<string, unknown> = {
    op: "task.update",
    task_id: task.id,
    title: String(task.title || ""),
    outcome: String(task.outcome || ""),
    assignee: task.assignee || null,
    priority: String(task.priority || ""),
    parent_id: task.parent_id || null,
    blocked_by: Array.isArray(task.blocked_by) ? task.blocked_by : [],
    waiting_on: String(task.waiting_on || "none"),
    handoff_to: task.handoff_to || null,
    notes: String(task.notes || ""),
    checklist: Array.isArray(task.checklist)
      ? task.checklist.map((item, index) => ({
          id: String(item.id || `item-${index + 1}`),
          text: String(item.text || ""),
          status: String(item.status || "pending"),
        }))
      : [],
  };
  const ops: Array<Record<string, unknown>> = [updateOp];
  if (task.status) {
    ops.push({ op: "task.move", task_id: task.id, status: String(task.status || "planned") });
  }
  return contextSync(groupId, ops);
}


export async function deleteCoordinationTask(groupId: string, taskId: string) {
  return contextSync(groupId, [{ op: "task.delete", task_id: String(taskId || "") }]);
}


export async function fetchSettings(groupId: string) {
  return apiJson<{ settings: GroupSettings }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/settings`
  );
}

export async function fetchCapabilityOverview(opts?: { query?: string; limit?: number; includeIndexed?: boolean }) {
  const params = new URLSearchParams();
  if (String(opts?.query || "").trim()) params.set("query", String(opts?.query || "").trim());
  if (typeof opts?.limit === "number" && Number.isFinite(opts.limit)) params.set("limit", String(Math.max(1, Math.trunc(opts.limit))));
  if (typeof opts?.includeIndexed === "boolean") params.set("include_indexed", opts.includeIndexed ? "true" : "false");
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return apiJson<{
    items: CapabilityOverviewItem[];
    count: number;
    query?: string;
    sources?: Record<string, CapabilitySourceState>;
    blocked_capabilities?: CapabilityBlockEntry[];
    allowlist_revision?: string;
  }>(`/api/v1/capabilities/overview${suffix}`);
}

export async function fetchCapabilityAllowlist() {
  return apiJson<{
    revision: string;
    default: Record<string, unknown>;
    overlay: Record<string, unknown>;
    effective: Record<string, unknown>;
    default_source?: string;
    overlay_source?: string;
    overlay_error?: string;
    policy_source?: string;
    policy_error?: string;
    external_capability_safety_mode?: string;
  }>(`/api/v1/capabilities/allowlist?by=user`);
}

export async function updateCapabilityAllowlist(args: {
  patch?: Record<string, unknown>;
  overlay?: Record<string, unknown>;
  mode?: "patch" | "replace";
  expectedRevision?: string;
}) {
  const body: Record<string, unknown> = { by: "user", mode: args.mode || "patch" };
  if (args.patch) body.patch = args.patch;
  if (args.overlay) body.overlay = args.overlay;
  if (String(args.expectedRevision || "").trim()) body.expected_revision = String(args.expectedRevision || "").trim();
  return apiJson<{
    updated: boolean;
    revision: string;
    default: Record<string, unknown>;
    overlay: Record<string, unknown>;
    effective: Record<string, unknown>;
    policy_source?: string;
    policy_error?: string;
    external_capability_safety_mode?: string;
  }>(`/api/v1/capabilities/allowlist`, {
    method: "PUT",
    body: JSON.stringify(body),
  });
}

export async function blockCapabilityGlobal(capabilityId: string, blocked: boolean, reason: string = "", groupId?: string) {
  const body: Record<string, unknown> = {
    by: "user",
    actor_id: "user",
    capability_id: capabilityId,
    blocked,
    reason,
    scope: "global",
  };
  if (String(groupId || "").trim()) body.group_id = String(groupId || "").trim();
  return apiJson<Record<string, unknown>>(`/api/v1/capabilities/block`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function fetchGroupCapabilityState(groupId: string, actorId: string = "user") {
  const params = new URLSearchParams();
  if (actorId) params.set("actor_id", actorId);
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return apiJson<CapabilityStateResult>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/state${suffix}`
  );
}

export async function enableGroupCapability(
  groupId: string,
  capabilityId: string,
  opts?: {
    enabled?: boolean;
    scope?: "session" | "actor" | "group";
    actorId?: string;
    ttlSeconds?: number;
    reason?: string;
    cleanup?: boolean;
  }
) {
  return apiJson<Record<string, unknown>>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/enable`,
    {
      method: "POST",
      body: JSON.stringify({
        capability_id: capabilityId,
        enabled: opts?.enabled ?? true,
        scope: opts?.scope || "session",
        actor_id: opts?.actorId || "user",
        ttl_seconds: opts?.ttlSeconds || 3600,
        reason: opts?.reason || "",
        cleanup: opts?.cleanup || false,
      }),
    }
  );
}

export async function importCapability(
  groupId: string,
  record: CapabilityImportRecord,
  opts?: {
    dryRun?: boolean;
    enableAfterImport?: boolean;
    scope?: "session" | "actor" | "group";
    actorId?: string;
    ttlSeconds?: number;
    reason?: string;
  }
) {
  return apiJson<Record<string, unknown>>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/import`,
    {
      method: "POST",
      body: JSON.stringify({
        record,
        dry_run: opts?.dryRun || false,
        enable_after_import: opts?.enableAfterImport || false,
        scope: opts?.scope || "session",
        actor_id: opts?.actorId || "user",
        ttl_seconds: opts?.ttlSeconds || 3600,
        reason: opts?.reason || "",
      }),
    }
  );
}

export async function updateSettings(groupId: string, settings: Partial<GroupSettings>) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/settings`, {
    method: "PUT",
    body: JSON.stringify({ ...settings, by: "user" }),
  });
}

export async function fetchAutomation(groupId: string) {
  return apiJson<GroupAutomation>(`/api/v1/groups/${encodeURIComponent(groupId)}/automation`);
}

export async function updateAutomation(groupId: string, ruleset: AutomationRuleSet, expectedVersion?: number) {
  const body: Record<string, unknown> = { rules: ruleset.rules, snippets: ruleset.snippets, by: "user" };
  if (typeof expectedVersion === "number" && Number.isFinite(expectedVersion)) {
    body.expected_version = Math.trunc(expectedVersion);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/automation`, {
    method: "PUT",
    body: JSON.stringify(body),
  });
}

export async function resetAutomationBaseline(groupId: string, expectedVersion?: number) {
  const body: Record<string, unknown> = { by: "user" };
  if (typeof expectedVersion === "number" && Number.isFinite(expectedVersion)) {
    body.expected_version = Math.trunc(expectedVersion);
  }
  return apiJson<GroupAutomation>(`/api/v1/groups/${encodeURIComponent(groupId)}/automation/reset_baseline`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

// ============ Inbox ============

export async function fetchInbox(groupId: string, actorId: string, limit = 200) {
  return apiJson<{ messages: LedgerEvent[] }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/inbox/${encodeURIComponent(actorId)}?by=user&limit=${limit}`
  );
}

export async function markInboxRead(groupId: string, actorId: string, eventId: string) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/inbox/${encodeURIComponent(actorId)}/read`,
    {
      method: "POST",
      body: JSON.stringify({ event_id: eventId, by: "user" }),
    }
  );
}

// ============ Messages ============

export async function sendMessage(
  groupId: string,
  text: string,
  to: string[],
  files?: File[],
  priority: "normal" | "attention" = "normal",
  replyRequired = false,
  clientId = "",
  refs?: MessageRef[],
) {
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("to_json", JSON.stringify(to));
    form.append("path", "");
    form.append("priority", priority);
    form.append("reply_required", replyRequired ? "true" : "false");
    if (clientId) form.append("client_id", clientId);
    if (refs && refs.length > 0) form.append("refs_json", JSON.stringify(refs));
    for (const f of files) form.append("files", f);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/send_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
    method: "POST",
    body: JSON.stringify({ text, by: "user", to, path: "", priority, reply_required: replyRequired, client_id: clientId, refs: refs || [] }),
  });
}

export async function replyMessage(
  groupId: string,
  text: string,
  to: string[],
  replyTo: string,
  files?: File[],
  priority: "normal" | "attention" = "normal",
  replyRequired = false,
  clientId = "",
  refs?: MessageRef[],
) {
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("to_json", JSON.stringify(to));
    form.append("reply_to", replyTo);
    form.append("priority", priority);
    form.append("reply_required", replyRequired ? "true" : "false");
    if (clientId) form.append("client_id", clientId);
    if (refs && refs.length > 0) form.append("refs_json", JSON.stringify(refs));
    for (const f of files) form.append("files", f);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/reply_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/reply`, {
    method: "POST",
    body: JSON.stringify({ text, by: "user", to, reply_to: replyTo, priority, reply_required: replyRequired, client_id: clientId, refs: refs || [] }),
  });
}

export async function relayMessage(
  dstGroupId: string,
  text: string,
  to: string[],
  src: { groupId: string; eventId: string }
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(dstGroupId)}/send`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      to,
      path: "",
      priority: "normal",
      src_group_id: src.groupId,
      src_event_id: src.eventId,
    }),
  });
}

export async function sendCrossGroupMessage(
  srcGroupId: string,
  dstGroupId: string,
  text: string,
  to: string[],
  priority: "normal" | "attention" = "normal",
  replyRequired = false
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(srcGroupId)}/send_cross_group`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      dst_group_id: dstGroupId,
      to,
      priority,
      reply_required: replyRequired,
    }),
  });
}

// ============ Runtimes ============

export async function fetchRuntimes() {
  return apiJson<{ runtimes: RuntimeInfo[]; available: string[] }>("/api/v1/runtimes");
}

// ============ File System ============

export async function fetchDirSuggestions() {
  return apiJson<{ suggestions: DirSuggestion[] }>("/api/v1/fs/recent");
}

export async function fetchDirContents(path: string) {
  return apiJson<{ path: string; parent: string | null; items: DirItem[] }>(
    `/api/v1/fs/list?path=${encodeURIComponent(path)}`
  );
}

export async function resolveScopeRoot(path: string) {
  return apiJson<{ path: string; scope_root: string; scope_key: string; git_remote: string }>(
    `/api/v1/fs/scope_root?path=${encodeURIComponent(path)}`
  );
}

// ============ IM Bridge ============

export async function fetchIMStatus(groupId: string) {
  return apiJson<IMStatus>(`/api/im/status?group_id=${encodeURIComponent(groupId)}`);
}

export async function fetchIMConfig(groupId: string) {
  return apiJson<{ im: IMConfig | null }>(`/api/im/config?group_id=${encodeURIComponent(groupId)}`);
}

export async function setIMConfig(
  groupId: string,
  platform: IMPlatform,
  botTokenEnv: string,
  appTokenEnv?: string,
  extra?: {
    feishu_domain?: string;
    feishu_app_id?: string;
    feishu_app_secret?: string;
    dingtalk_app_key?: string;
    dingtalk_app_secret?: string;
    dingtalk_robot_code?: string;
    wecom_bot_id?: string;
    wecom_secret?: string;
  }
) {
  const body: Record<string, unknown> = {
    group_id: groupId,
    platform,
  };

  // Telegram/Slack/Discord use bot_token_env
  if (platform === "telegram" || platform === "slack" || platform === "discord") {
    body.bot_token_env = botTokenEnv;
    if (platform === "slack" && appTokenEnv) {
      body.app_token_env = appTokenEnv;
    }
  }

  // Feishu uses app_id and app_secret
  if (platform === "feishu" && extra) {
    body.feishu_domain = extra.feishu_domain;
    body.feishu_app_id = extra.feishu_app_id;
    body.feishu_app_secret = extra.feishu_app_secret;
  }

  // DingTalk uses app_key and app_secret
  if (platform === "dingtalk" && extra) {
    body.dingtalk_app_key = extra.dingtalk_app_key;
    body.dingtalk_app_secret = extra.dingtalk_app_secret;
    body.dingtalk_robot_code = extra.dingtalk_robot_code;
  }

  // WeCom uses bot_id / secret
  if (platform === "wecom" && extra) {
    body.wecom_bot_id = extra.wecom_bot_id;
    body.wecom_secret = extra.wecom_secret;
  }

  return apiJson("/api/im/set", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function unsetIMConfig(groupId: string) {
  return apiJson("/api/im/unset", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId }),
  });
}

export async function startIMBridge(groupId: string) {
  return apiJson("/api/im/start", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId }),
  });
}

export async function stopIMBridge(groupId: string) {
  return apiJson("/api/im/stop", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId }),
  });
}

export interface IMAuthorizedChat {
  chat_id: string;
  thread_id: number;
  platform: string;
  authorized_at: number;
  key_used?: string;
  verbose?: boolean;
}

export interface IMPendingRequest {
  key: string;
  chat_id: string;
  thread_id: number;
  platform: string;
  created_at: number;
  expires_at: number;
  expires_in_seconds: number;
}

export async function fetchIMAuthorized(groupId: string) {
  return apiJson<{ authorized: IMAuthorizedChat[] }>(
    `/api/im/authorized?group_id=${encodeURIComponent(groupId)}`
  );
}

export async function fetchIMPending(groupId: string) {
  return apiJson<{ pending: IMPendingRequest[] }>(
    `/api/im/pending?group_id=${encodeURIComponent(groupId)}`
  );
}

export async function revokeIMChat(groupId: string, chatId: string, threadId: number = 0) {
  return apiJson(`/api/im/revoke?group_id=${encodeURIComponent(groupId)}&chat_id=${encodeURIComponent(chatId)}&thread_id=${threadId}`, {
    method: "POST",
  });
}

export async function rejectIMPending(groupId: string, key: string) {
  return apiJson<{ rejected: boolean }>("/api/im/pending/reject", {
    method: "POST",
    body: JSON.stringify({ group_id: groupId, key }),
  });
}

export async function bindIMChat(groupId: string, key: string) {
  return apiJson<{ chat_id: string; thread_id: number; platform: string }>(
    "/api/im/bind",
    { method: "POST", body: JSON.stringify({ group_id: groupId, key }) },
  );
}

export async function setIMVerbose(groupId: string, chatId: string, verbose: boolean, threadId: number = 0) {
  return apiJson<{ chat_id: string; thread_id: number; verbose: boolean }>(
    `/api/im/verbose?group_id=${encodeURIComponent(groupId)}&chat_id=${encodeURIComponent(chatId)}&verbose=${verbose}&thread_id=${threadId}`,
    { method: "POST" },
  );
}

// ============ Observability ============

export interface Observability {
  developer_mode?: boolean;
  log_level?: string;
  terminal_transcript?: {
    enabled?: boolean;
    per_actor_bytes?: number;
    persist?: boolean;
    strip_ansi?: boolean;
  };
  terminal_ui?: {
    scrollback_lines?: number;
  };
}

export async function fetchObservability() {
  return apiJson<{ observability: Observability }>("/api/v1/observability");
}

export async function updateObservability(args: {
  developerMode: boolean;
  logLevel: "INFO" | "DEBUG";
  terminalTranscriptPerActorBytes?: number;
  terminalUiScrollbackLines?: number;
}) {
  return apiJson<{ observability: Observability }>("/api/v1/observability", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      developer_mode: args.developerMode,
      log_level: args.logLevel,
      terminal_transcript_per_actor_bytes: args.terminalTranscriptPerActorBytes,
      terminal_ui_scrollback_lines: args.terminalUiScrollbackLines,
    }),
  });
}

export async function fetchRemoteAccessState() {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access");
}

export async function fetchWebAccessSession() {
  return reuseRecentReadRequest(
    webAccessSessionRequestKey(),
    RECENT_BOOTSTRAP_READ_TTL_MS,
    () => apiJson<{ web_access_session: WebAccessSession }>("/api/v1/web_access/session")
  );
}

export async function fetchWebBranding() {
  return apiJson<{ branding: WebBranding }>("/api/v1/branding");
}

export async function updateWebBranding(args: {
  productName?: string;
  clearLogoIcon?: boolean;
  clearFavicon?: boolean;
}) {
  return apiJson<{ branding: WebBranding }>("/api/v1/branding", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      product_name: args.productName,
      clear_logo_icon: Boolean(args.clearLogoIcon),
      clear_favicon: Boolean(args.clearFavicon),
    }),
  });
}

export async function uploadWebBrandingAsset(assetKind: "logo_icon" | "favicon", file: File) {
  const form = new FormData();
  form.append("by", "user");
  form.append("file", file);
  return apiForm<{ branding: WebBranding }>(`/api/v1/branding/assets/${assetKind}`, form);
}

export async function clearWebBrandingAsset(assetKind: "logo_icon" | "favicon") {
  return apiJson<{ branding: WebBranding }>(`/api/v1/branding/assets/${assetKind}?by=user`, {
    method: "DELETE",
  });
}

export async function logoutWebAccess() {
  clearWebAccessSessionReadRequest();
  return apiJson<{ signed_out: boolean }>("/api/v1/web_access/logout", {
    method: "POST",
  });
}

export async function updateRemoteAccessConfig(args: {
  provider?: "off" | "manual" | "tailscale";
  mode?: string;
  enabled?: boolean;
  requireAccessToken?: boolean;
  webHost?: string;
  webPort?: number;
  webPublicUrl?: string;
}) {
  clearPingReadRequest();
  clearWebAccessSessionReadRequest();
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      provider: args.provider,
      mode: args.mode,
      enabled: args.enabled,
      require_access_token: args.requireAccessToken,
      web_host: args.webHost,
      web_port: args.webPort,
      web_public_url: args.webPublicUrl,
    }),
  });
}

export async function startRemoteAccess() {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access/start?by=user", {
    method: "POST",
  });
}

export async function stopRemoteAccess() {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access/stop?by=user", {
    method: "POST",
  });
}

export async function applyRemoteAccess() {
  clearPingReadRequest();
  clearWebAccessSessionReadRequest();
  return apiJson<{ accepted: boolean; remote_access: RemoteAccessState; target_local_url?: string | null; target_remote_url?: string | null }>(
    "/api/v1/remote_access/apply?by=user",
    {
      method: "POST",
    }
  );
}

// ============ Access token management ============

export interface AccessTokenEntry {
  token?: string;
  token_id?: string;
  token_preview?: string;
  user_id: string;
  is_admin: boolean;
  allowed_groups: string[];
  created_at: string;
}

export async function fetchAccessTokens() {
  return apiJson<{ access_tokens: AccessTokenEntry[] }>("/api/v1/access-tokens");
}

export async function createAccessToken(userId: string, isAdmin: boolean, allowedGroups: string[], customToken?: string) {
  clearWebAccessSessionReadRequest();
  const body: Record<string, unknown> = {
    user_id: userId,
    is_admin: isAdmin,
    allowed_groups: allowedGroups,
  };
  if (customToken?.trim()) body.custom_token = customToken.trim();
  return apiJson<{ access_token: AccessTokenEntry }>("/api/v1/access-tokens", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function updateAccessToken(tokenId: string, updates: { allowed_groups?: string[]; is_admin?: boolean }) {
  clearWebAccessSessionReadRequest();
  return apiJson<{ access_token: AccessTokenEntry }>(`/api/v1/access-tokens/${encodeURIComponent(tokenId)}`, {
    method: "PATCH",
    body: JSON.stringify(updates),
  });
}

export async function revealAccessToken(tokenId: string) {
  return apiJson<{ token: string }>(`/api/v1/access-tokens/${encodeURIComponent(tokenId)}/reveal`);
}

export async function deleteAccessToken(tokenId: string) {
  clearWebAccessSessionReadRequest();
  return apiJson<{ deleted: boolean; access_tokens_remain?: boolean; deleted_current_session?: boolean }>(`/api/v1/access-tokens/${encodeURIComponent(tokenId)}`, {
    method: "DELETE",
  });
}

export async function fetchGroupSpaceStatus(groupId: string, provider: string = "notebooklm") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/status?provider=${encodeURIComponent(provider)}`
  );
}

export async function bindGroupSpace(groupId: string, remoteSpaceId: string = "", provider: string = "notebooklm", lane: "work" | "memory") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/bind`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        provider,
        lane,
        action: "bind",
        remote_space_id: String(remoteSpaceId || ""),
      }),
    }
  );
}

export async function fetchGroupSpaceSpaces(groupId: string, provider: string = "notebooklm") {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_state?: Record<string, unknown>;
    bindings?: Record<string, Record<string, unknown>>;
    spaces: GroupSpaceRemoteSpace[];
  }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/spaces?provider=${encodeURIComponent(provider)}`
  );
}

export async function unbindGroupSpace(groupId: string, provider: string = "notebooklm", lane: "work" | "memory") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/bind`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        provider,
        lane,
        action: "unbind",
        remote_space_id: "",
      }),
    }
  );
}

export async function ingestGroupSpace(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  kind: "context_sync" | "resource_ingest";
  payload: Record<string, unknown>;
  idempotencyKey?: string;
}) {
  return apiJson<{
    group_id: string;
    job_id: string;
    accepted: boolean;
    deduped: boolean;
    job: GroupSpaceJob;
    queue_summary: { pending: number; running: number; failed: number };
    provider_mode: string;
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/ingest`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider: args.provider || "notebooklm",
      lane: args.lane,
      kind: args.kind,
      payload: args.payload || {},
      idempotency_key: String(args.idempotencyKey || ""),
    }),
  });
}

export async function queryGroupSpace(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  query: string;
  options?: Record<string, unknown>;
}) {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    degraded: boolean;
    answer: string;
    references: unknown[];
    error?: { code?: string; message?: string } | null;
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/query`, {
    method: "POST",
    body: JSON.stringify({
      provider: args.provider || "notebooklm",
      lane: args.lane,
      query: args.query,
      options: args.options || {},
    }),
  });
}

export async function fetchGroupSpaceSources(groupId: string, provider: string = "notebooklm", lane: "work" | "memory") {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    binding?: Record<string, unknown>;
    action: "list";
    sources: GroupSpaceSource[];
    list_result?: Record<string, unknown>;
  }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/sources?provider=${encodeURIComponent(provider)}&lane=${encodeURIComponent(lane)}`
  );
}

export async function actionGroupSpaceSource(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  action: "delete" | "rename" | "refresh";
  sourceId: string;
  newTitle?: string;
}) {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    binding?: Record<string, unknown>;
    action: string;
    source_id: string;
    delete_result?: Record<string, unknown>;
    rename_result?: Record<string, unknown>;
    refresh_result?: Record<string, unknown>;
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/sources`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider: args.provider || "notebooklm",
      lane: args.lane,
      action: args.action,
      source_id: args.sourceId,
      new_title: String(args.newTitle || ""),
    }),
  });
}

export async function fetchGroupSpaceArtifacts(
  groupId: string,
  provider: string = "notebooklm",
  lane: "work" | "memory",
  kind: string = ""
) {
  const params = new URLSearchParams({
    provider: String(provider || "notebooklm"),
    lane: String(lane),
  });
  if (String(kind || "").trim()) {
    params.set("kind", String(kind || "").trim().toLowerCase());
  }
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    binding?: Record<string, unknown>;
    action: "list";
    kind?: string;
    artifacts: GroupSpaceArtifact[];
    list_result?: Record<string, unknown>;
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/space/artifacts?${params.toString()}`);
}

export async function actionGroupSpaceArtifact(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  action: "generate" | "download";
  kind: string;
  options?: Record<string, unknown>;
  wait?: boolean;
  saveToSpace?: boolean;
  outputPath?: string;
  outputFormat?: string;
  artifactId?: string;
  timeoutSeconds?: number;
  initialInterval?: number;
  maxInterval?: number;
}) {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    binding?: Record<string, unknown>;
    action: string;
    kind: string;
    task_id?: string;
    status?: string;
    wait?: boolean;
    saved_to_space?: boolean;
    output_path?: string;
    generate_result?: Record<string, unknown>;
    wait_result?: Record<string, unknown>;
    download_result?: Record<string, unknown>;
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/artifacts`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider: args.provider || "notebooklm",
      lane: args.lane,
      action: args.action,
      kind: String(args.kind || "").trim().toLowerCase(),
      options: args.options || {},
      wait: args.wait ?? true,
      save_to_space: args.saveToSpace ?? true,
      output_path: String(args.outputPath || ""),
      output_format: String(args.outputFormat || ""),
      artifact_id: String(args.artifactId || ""),
      timeout_seconds: Number(args.timeoutSeconds || 600),
      initial_interval: Number(args.initialInterval || 2),
      max_interval: Number(args.maxInterval || 10),
    }),
  });
}

export async function listGroupSpaceJobs(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  state?: string;
  limit?: number;
}) {
  const params = new URLSearchParams({
    provider: String(args.provider || "notebooklm"),
    lane: String(args.lane),
    limit: String(Math.max(1, Math.min(500, Number(args.limit || 50)))),
  });
  if (String(args.state || "").trim()) {
    params.set("state", String(args.state || "").trim());
  }
  return apiJson<{
    group_id: string;
    provider: string;
    jobs: GroupSpaceJob[];
    queue_summary: { pending: number; running: number; failed: number };
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/jobs?${params.toString()}`);
}

export async function actionGroupSpaceJob(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  action: "retry" | "cancel";
  jobId: string;
}) {
  return apiJson<{
    group_id: string;
    provider: string;
    job: GroupSpaceJob;
    queue_summary: { pending: number; running: number; failed: number };
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/jobs`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider: args.provider || "notebooklm",
      lane: args.lane,
      action: args.action,
      job_id: args.jobId,
    }),
  });
}

export async function syncGroupSpace(args: {
  groupId: string;
  provider?: string;
  lane: "work" | "memory";
  action?: "status" | "run";
  force?: boolean;
}) {
  return apiJson<{
    group_id: string;
    provider: string;
    sync?: Record<string, unknown>;
    sync_result?: Record<string, unknown>;
  }>(`/api/v1/groups/${encodeURIComponent(args.groupId)}/space/sync`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider: args.provider || "notebooklm",
      lane: args.lane,
      action: args.action || "run",
      force: Boolean(args.force),
    }),
  });
}

export async function fetchGroupSpaceProviderCredential(provider: string = "notebooklm") {
  return apiJson<{ provider: string; credential: GroupSpaceProviderCredentialStatus }>(
    `/api/v1/space/providers/${encodeURIComponent(provider)}/credential?by=user`
  );
}

export async function updateGroupSpaceProviderCredential(args: {
  provider?: string;
  authJson?: string;
  clear?: boolean;
}) {
  const provider = String(args.provider || "notebooklm");
  return apiJson<{ provider: string; credential: GroupSpaceProviderCredentialStatus }>(
    `/api/v1/space/providers/${encodeURIComponent(provider)}/credential`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        auth_json: String(args.authJson || ""),
        clear: Boolean(args.clear),
      }),
    }
  );
}

export async function checkGroupSpaceProviderHealth(provider: string = "notebooklm") {
  return apiJson<{
    provider: string;
    healthy: boolean;
    health?: Record<string, unknown>;
    error?: { code?: string; message?: string };
    provider_state?: Record<string, unknown>;
    credential?: GroupSpaceProviderCredentialStatus;
  }>(`/api/v1/space/providers/${encodeURIComponent(provider)}/health?by=user`, {
    method: "POST",
  });
}

export async function controlGroupSpaceProviderAuth(args: {
  provider?: string;
  action: "status" | "start" | "cancel" | "disconnect";
  timeoutSeconds?: number;
  forceReauth?: boolean;
  projected?: boolean;
}) {
  const provider = args.provider || "notebooklm";
  if (args.action === "status") {
    return apiJson<{
      provider: string;
      provider_state: Record<string, unknown>;
      credential: GroupSpaceProviderCredentialStatus;
      auth: GroupSpaceProviderAuthStatus;
    }>(`/api/v1/space/providers/${encodeURIComponent(provider)}/auth?by=user`, {
      method: "GET",
    });
  }
  return apiJson<{
    provider: string;
    provider_state: Record<string, unknown>;
    credential: GroupSpaceProviderCredentialStatus;
    auth: GroupSpaceProviderAuthStatus;
  }>(`/api/v1/space/providers/${encodeURIComponent(provider)}/auth`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      action: args.action,
      timeout_seconds: Number(args.timeoutSeconds || 900),
      force_reauth: Boolean(args.forceReauth),
      projected: Boolean(args.projected),
    }),
  });
}

export async function fetchGroupSpaceProviderAuthBrowserSession(
  provider: string = "notebooklm",
): Promise<ApiResponse<{ provider: string; browser_surface: PresentationBrowserSurfaceState }>> {
  const resp = await controlGroupSpaceProviderAuth({ provider, action: "status" });
  if (!resp.ok) {
    return resp as ApiResponse<{ provider: string; browser_surface: PresentationBrowserSurfaceState }>;
  }
  return {
    ok: true,
    result: {
      provider,
      browser_surface: normalizePresentationBrowserSurfaceState(resp.result.auth?.projected_browser),
    },
  };
}

export function getGroupSpaceProviderAuthBrowserWebSocketUrl(provider: string = "notebooklm"): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  const base = `${protocol}//${window.location.host}/api/v1/space/providers/${encodeURIComponent(provider)}/auth/browser_surface/ws`;
  return withAuthToken(base);
}

export interface RegistryReconcileResult {
  dry_run: boolean;
  scanned_groups: number;
  missing_group_ids: string[];
  corrupt_group_ids: string[];
  removed_group_ids: string[];
  removed_default_scope_keys: string[];
}

export async function previewRegistryReconcile() {
  return apiJson<RegistryReconcileResult>("/api/v1/registry/reconcile");
}

export async function executeRegistryReconcile(removeMissing = true) {
  return apiJson<RegistryReconcileResult>("/api/v1/registry/reconcile", {
    method: "POST",
    body: JSON.stringify({ by: "user", remove_missing: !!removeMissing }),
  });
}

// ============ Debug ============

export async function fetchDebugSnapshot(groupId: string) {
  return apiJson<Record<string, unknown>>(`/api/v1/debug/snapshot?group_id=${encodeURIComponent(groupId)}`);
}

export async function fetchTerminalTail(
  groupId: string,
  actorId: string,
  maxChars = 8000,
  stripAnsi = true,
  compact = true
) {
  const params = new URLSearchParams({
    actor_id: actorId,
    max_chars: String(maxChars),
    strip_ansi: String(stripAnsi),
    compact: String(compact),
  });
  return apiJson<{ text: string; hint: string }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/terminal/tail?${params.toString()}`
  );
}

export async function clearTerminalTail(groupId: string, actorId: string) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/terminal/clear?actor_id=${encodeURIComponent(actorId)}`,
    { method: "POST" }
  );
}

export async function fetchLogTail(component: "daemon" | "web" | "im", groupId: string, lines = 200) {
  const params = new URLSearchParams({
    component,
    group_id: groupId,
    lines: String(lines),
  });
  return apiJson<{ lines: string[] }>(`/api/v1/debug/tail_logs?${params.toString()}`);
}

export async function clearLogs(component: "daemon" | "web" | "im", groupId: string) {
  return apiJson("/api/v1/debug/clear_logs", {
    method: "POST",
    body: JSON.stringify({ component, group_id: groupId, by: "user" }),
  });
}

export async function exportFeedbackBundle(groupId: string) {
  return apiJson<{ attachment?: { path?: string; title?: string; mime_type?: string; bytes?: number } }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/feedback_bundle/export`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user" }),
    }
  );
}
