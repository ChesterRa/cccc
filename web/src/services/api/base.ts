import type {
  AgentState,
  ActorRuntimeState,
  ContextDetailLevel,
  GroupContext,
  GroupPresentation,
  GroupTasksSummary,
  PresentationBrowserSurfaceState,
  PresentationCard,
  PresentationCardType,
  PresentationContent,
  PresentationSlot,
  PresentationTableData,
  Task,
  TaskBoardEntry,
  TaskChecklistItem,
} from "../../types";

export type ApiResponse<T> =
  | { ok: true; result: T; error?: null }
  | { ok: false; result?: unknown; error: { code: string; message: string; details?: unknown } };

export type FetchContextOptions = {
  fresh?: boolean;
  detail?: ContextDetailLevel;
  noCache?: boolean;
  signal?: AbortSignal;
};

type SharedReadPromise = Promise<ApiResponse<unknown>>;
type RecentReadEntry = {
  response: ApiResponse<unknown>;
  expiresAt: number;
};

type UnknownRecord = Record<string, unknown>;
type ApiErrorShape = {
  code?: string;
  message?: string;
  details?: unknown;
};

let cachedToken: string | null = null;
const FORCE_LOGIN_KEY = "cccc_force_token_login";

let authRequiredHandler: (() => void) | null = null;

const sharedReadRequests = new Map<string, SharedReadPromise>();
const recentReadResponses = new Map<string, RecentReadEntry>();
const recentReadGenerations = new Map<string, number>();

export const RECENT_BOOTSTRAP_READ_TTL_MS = 1000;

let globalReadEpoch = 0;

export function onAuthRequired(handler: () => void): void {
  authRequiredHandler = handler;
}

function clearAllReadRequestCaches(): void {
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

export function withAuthToken(url: string): string {
  const token = getAuthToken();
  if (!token) return url;
  const sep = url.includes("?") ? "&" : "?";
  return `${url}${sep}token=${encodeURIComponent(token)}`;
}

function makeErrorResponse<T>(code: string, message: string): ApiResponse<T> {
  return { ok: false, error: { code, message } };
}

export function reuseSharedReadRequest<T>(
  key: string,
  loader: () => Promise<ApiResponse<T>>,
): Promise<ApiResponse<T>> {
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

export function reuseRecentReadRequest<T>(
  key: string,
  ttlMs: number,
  loader: () => Promise<ApiResponse<T>>,
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

export function clearSharedReadRequest(key: string): void {
  sharedReadRequests.delete(key);
}

export function clearRecentReadRequest(key: string): void {
  recentReadGenerations.set(key, (recentReadGenerations.get(key) || 0) + 1);
  recentReadResponses.delete(key);
  clearSharedReadRequest(key);
}

export function actorsReadOnlyRequestKey(groupId: string, includeInternal = false): string {
  return `actors:${String(groupId || "").trim()}:read-only:${includeInternal ? "internal" : "standard"}`;
}

export function groupsRequestKey(): string {
  return "groups:list";
}

export function groupPromptsRequestKey(groupId: string): string {
  return `group-prompts:${String(groupId || "").trim()}`;
}

export function petPeerContextRequestKey(groupId: string, fresh: boolean, verbose: boolean): string {
  return `pet-peer-context:${String(groupId || "").trim()}:${fresh ? "fresh" : "default"}:${verbose ? "verbose" : "lite"}`;
}

export function ledgerStatusesRequestKey(groupId: string, eventIds: string[]): string {
  const normalizedIds = eventIds.map((eventId) => String(eventId || "").trim()).filter((eventId) => eventId);
  return `ledger-statuses:${String(groupId || "").trim()}:${normalizedIds.join(",")}`;
}

export function pingRequestKey(includeHome: boolean): string {
  return includeHome ? "ping:include-home" : "ping:default";
}

export function webAccessSessionRequestKey(): string {
  return "web-access-session";
}

export function contextRequestKey(groupId: string, detail: ContextDetailLevel): string {
  return `context:${String(groupId || "").trim()}:${detail}`;
}

export function capabilityStateRequestKey(groupId: string, actorId: string, capabilityId: string = "", view: string = ""): string {
  return [
    "capability-state",
    String(groupId || "").trim(),
    String(actorId || "user").trim() || "user",
    String(capabilityId || "").trim(),
    String(view || "").trim(),
  ].join(":");
}

export function clearGroupsReadRequest(): void {
  clearRecentReadRequest(groupsRequestKey());
}

export function invalidateGroupsRead(): void {
  clearGroupsReadRequest();
}

export function clearPingReadRequest(includeHome?: boolean): void {
  if (typeof includeHome === "boolean") {
    clearRecentReadRequest(pingRequestKey(includeHome));
    return;
  }
  clearRecentReadRequest(pingRequestKey(false));
  clearRecentReadRequest(pingRequestKey(true));
}

export function clearWebAccessSessionReadRequest(): void {
  clearRecentReadRequest(webAccessSessionRequestKey());
}

export function clearActorsReadOnlyRequest(groupId: string): void {
  clearSharedReadRequest(actorsReadOnlyRequestKey(groupId, false));
  clearSharedReadRequest(actorsReadOnlyRequestKey(groupId, true));
}

export function clearContextRequest(groupId: string, detail?: ContextDetailLevel): void {
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

export function asRecord(value: unknown): UnknownRecord | null {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as UnknownRecord) : null;
}

export function asString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

export function asOptionalString(value: unknown): string | null {
  const text = asString(value).trim();
  return text ? text : null;
}

export function asStringArray(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.map((item) => asString(item).trim()).filter(Boolean);
}

function formatDaemonEndpoint(details: UnknownRecord | null): string {
  const endpoint = asRecord(details?.endpoint);
  if (!endpoint) return "";
  const path = asString(endpoint.path).trim();
  if (path) return path;
  const host = asString(endpoint.host).trim();
  const port = asString(endpoint.port).trim();
  if (host && port) return `${host}:${port}`;
  return host || port;
}

function formatDaemonReason(details: UnknownRecord | null): string {
  const phase = asString(details?.phase).trim();
  const reason = asString(details?.reason).trim().replace(/_/g, " ");
  return [phase, reason].filter(Boolean).join(" ");
}

export function formatApiErrorMessage(error: ApiErrorShape): string {
  const code = asString(error.code).trim();
  const baseMessage = asString(error.message).trim() || "Request failed";
  if (code !== "daemon_unavailable") return baseMessage;

  const details = asRecord(error.details);
  const transport = asString(details?.transport).trim();
  const endpoint = formatDaemonEndpoint(details);
  const reason = formatDaemonReason(details);
  const diagnostics = [
    transport ? [transport, endpoint].filter(Boolean).join(" ") : endpoint,
    reason,
  ].filter(Boolean);
  return diagnostics.length > 0 ? `${baseMessage} · ${diagnostics.join(" · ")}` : baseMessage;
}

export function normalizeApiResponse<T>(data: unknown): ApiResponse<T> {
  const record = asRecord(data);
  if (!record) {
    return makeErrorResponse("PARSE_ERROR", "Invalid API response");
  }
  if (record.ok === false) {
    const errorRecord = asRecord(record.error);
    const code = asString(errorRecord?.code).trim() || "UNKNOWN_ERROR";
    const message = formatApiErrorMessage({
      code,
      message: asString(errorRecord?.message).trim() || "Request failed",
      details: errorRecord?.details,
    });
    return {
      ok: false,
      result: record.result,
      error: {
        code,
        message,
        details: errorRecord?.details,
      },
    };
  }
  return record as ApiResponse<T>;
}

export function normalizeChecklistItem(value: unknown, index: number): TaskChecklistItem | null {
  const record = asRecord(value);
  if (!record) return null;
  const text = asString(record.text).trim();
  if (!text) return null;
  const id = asString(record.id).trim() || `item-${index + 1}`;
  const status = asString(record.status).trim() || "pending";
  return { id, text, status };
}

export function normalizeTask(value: unknown): Task | null {
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
    task_type: asOptionalString(record.task_type),
    notes: asString(record.notes),
    checklist,
    created_at: asOptionalString(record.created_at),
    updated_at: asOptionalString(record.updated_at),
    archived_from: asOptionalString(record.archived_from),
    progress:
      typeof record.progress === "number"
        ? record.progress
        : checklist.length > 0
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

function normalizeActorRuntimeState(value: unknown): ActorRuntimeState | null {
  const record = asRecord(value);
  if (!record) return null;
  const id = asString(record.id).trim();
  if (!id) return null;
  return {
    id,
    runtime: asOptionalString(record.runtime) || undefined,
    runner: asOptionalString(record.runner) || undefined,
    runner_effective: asOptionalString(record.runner_effective) || undefined,
    running: typeof record.running === "boolean" ? record.running : undefined,
    idle_seconds: typeof record.idle_seconds === "number" ? record.idle_seconds : null,
    effective_working_state: asOptionalString(record.effective_working_state) || undefined,
    effective_working_reason: asOptionalString(record.effective_working_reason) || undefined,
    effective_working_updated_at: asOptionalString(record.effective_working_updated_at),
    effective_active_task_id: asOptionalString(record.effective_active_task_id),
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

export function normalizeContext(raw: unknown): GroupContext {
  const record = asRecord(raw) ?? {};
  const coordination = asRecord(record.coordination) ?? {};
  const tasks = Array.isArray(coordination.tasks)
    ? coordination.tasks.map((item) => normalizeTask(item)).filter((item): item is Task => !!item)
    : [];
  const agentStates = Array.isArray(record.agent_states)
    ? record.agent_states.map((item) => normalizeAgentState(item)).filter((item): item is AgentState => !!item)
    : [];
  const actorsRuntime = Array.isArray(record.actors_runtime)
    ? record.actors_runtime.map((item) => normalizeActorRuntimeState(item)).filter((item): item is ActorRuntimeState => !!item)
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
    actors_runtime: actorsRuntime,
    attention: attentionRecord
      ? {
          blocked: Array.isArray(attentionRecord.blocked)
            ? normalizeBoardEntries(attentionRecord.blocked) || []
            : typeof attentionRecord.blocked === "number"
              ? attentionRecord.blocked
              : undefined,
          waiting_user: Array.isArray(attentionRecord.waiting_user)
            ? normalizeBoardEntries(attentionRecord.waiting_user) || []
            : typeof attentionRecord.waiting_user === "number"
              ? attentionRecord.waiting_user
              : undefined,
          pending_handoffs: Array.isArray(attentionRecord.pending_handoffs)
            ? normalizeBoardEntries(attentionRecord.pending_handoffs) || []
            : typeof attentionRecord.pending_handoffs === "number"
              ? attentionRecord.pending_handoffs
              : undefined,
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
    ? record.rows.map((row) => (Array.isArray(row) ? row.map((cell) => asString(cell)) : []))
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

export function normalizePresentationCard(value: unknown): PresentationCard | null {
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

export function normalizePresentation(raw: unknown): GroupPresentation {
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

export function normalizePresentationBrowserSurfaceState(value: unknown): PresentationBrowserSurfaceState {
  const record = asRecord(value) ?? {};
  const errorRecord = asRecord(record.error);
  const viewerRecord = asRecord(record.viewer);
  const vncRecord = asRecord(viewerRecord?.vnc);
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
    viewer: viewerRecord
      ? {
          kind: asOptionalString(viewerRecord.kind),
          vnc: vncRecord
            ? {
                available: !!vncRecord.available,
                error: asOptionalString(vncRecord.error),
                started_at: asOptionalString(vncRecord.started_at),
              }
            : null,
        }
      : null,
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
  } catch (error) {
    return makeErrorResponse(
      "NETWORK_ERROR",
      error instanceof Error ? error.message : "Network request failed",
    );
  }

  if (resp.status === 401) {
    authRequiredHandler?.();
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
      authRequiredHandler?.();
    }
    return normalizeApiResponse<T>(data);
  } catch {
    if (!resp.ok) {
      const statusText = resp.statusText ? ` ${resp.statusText}` : "";
      return makeErrorResponse("HTTP_ERROR", `Server returned ${resp.status}${statusText}: ${text.slice(0, 100)}`);
    }
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
  } catch (error) {
    return makeErrorResponse(
      "NETWORK_ERROR",
      error instanceof Error ? error.message : "Network request failed",
    );
  }

  if (resp.status === 401) {
    authRequiredHandler?.();
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
      authRequiredHandler?.();
    }
    return normalizeApiResponse<T>(data);
  } catch {
    return makeErrorResponse("PARSE_ERROR", `Invalid JSON response: ${text.slice(0, 100)}`);
  }
}
