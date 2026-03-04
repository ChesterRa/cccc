// API service layer (centralized API calls).
import type {
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
} from "../types";

// ============ Token management ============

// Extract and cache token from URL for dev mode (Vite doesn't proxy /ui/ to backend)
let cachedToken: string | null = null;

// Global callback for 401 unauthorized responses
let _authRequiredHandler: (() => void) | null = null;

export function onAuthRequired(handler: () => void): void {
  _authRequiredHandler = handler;
}

export function setAuthToken(token: string): void {
  cachedToken = token;
  try {
    sessionStorage.setItem("cccc_dev_token", token);
  } catch {
    void 0;
  }
}

export function clearAuthToken(): void {
  cachedToken = "";
  try {
    sessionStorage.removeItem("cccc_dev_token");
  } catch {
    void 0;
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

// Helper to create a typed error response.
function makeErrorResponse<T>(code: string, message: string): ApiResponse<T> {
  return { ok: false, error: { code, message } };
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
  return apiJson<{ groups: GroupMeta[] }>("/api/v1/groups");
}

export async function fetchPing() {
  return apiJson<{ home: string; daemon: unknown; version: string; web?: { mode?: string; read_only?: boolean } }>(
    "/api/v1/ping"
  );
}

export async function fetchGroup(groupId: string) {
  return apiJson<{ group: GroupDoc }>(`/api/v1/groups/${encodeURIComponent(groupId)}`);
}

export async function createGroup(title: string, topic: string = "") {
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
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}`, {
    method: "PUT",
    body: JSON.stringify({ title: title.trim(), topic: topic.trim(), by: "user" }),
  });
}

export async function deleteGroup(groupId: string) {
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
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/start?by=user`, {
    method: "POST",
  });
}

export async function stopGroup(groupId: string) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/stop?by=user`, {
    method: "POST",
  });
}

export async function setGroupState(groupId: string, state: "active" | "idle" | "paused") {
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

export async function fetchActors(groupId: string, includeUnread = true) {
  const url = includeUnread
    ? `/api/v1/groups/${encodeURIComponent(groupId)}/actors?include_unread=true`
    : `/api/v1/groups/${encodeURIComponent(groupId)}/actors`;
  return apiJson<{ actors: Actor[] }>(url);
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
    runner?: "pty" | "headless";
    title?: string;
    capabilityAutoload?: string[];
  }
) {
  return apiJson<{ actor: Actor }>(`/api/v1/groups/${encodeURIComponent(groupId)}/actors`, {
    method: "POST",
    body: JSON.stringify({
      actor_id: actorId,
      role,
      runner: options?.runner || "pty",
      runtime,
      command,
      env: {},
      env_private: envPrivate && Object.keys(envPrivate).length ? envPrivate : undefined,
      profile_id: options?.profileId || undefined,
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
    profileAction?: "convert_to_custom";
    enabled?: boolean;
    capabilityAutoload?: string[];
  }
) {
  const body: Record<string, unknown> = { by: "user" };
  if (runtime !== undefined && runtime !== "") body.runtime = runtime;
  if (command !== undefined) body.command = command.trim();
  if (title !== undefined) body.title = title.trim();
  if (opts?.profileId !== undefined) body.profile_id = String(opts.profileId || "");
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

export async function attachActorProfile(groupId: string, actorId: string, profileId: string) {
  return apiJson<{ actor: Actor }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", profile_id: profileId }),
    }
  );
}

export async function convertActorToCustom(groupId: string, actorId: string) {
  return apiJson<{ actor: Actor }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", profile_action: "convert_to_custom" }),
    }
  );
}

export async function removeActor(groupId: string, actorId: string) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}?by=user`,
    { method: "DELETE" }
  );
}

export async function startActor(groupId: string, actorId: string) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/start`,
    { method: "POST" }
  );
}

export async function stopActor(groupId: string, actorId: string) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/stop`,
    { method: "POST" }
  );
}

export async function restartActor(groupId: string, actorId: string) {
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

export async function listActorProfiles() {
  return apiJson<{ profiles: ActorProfile[] }>(`/api/v1/actor_profiles?by=user`);
}

export async function getActorProfile(profileId: string) {
  return apiJson<{ profile: ActorProfile; usage: ActorProfileUsage[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}?by=user`
  );
}

export async function upsertActorProfile(profile: Record<string, unknown>, expectedRevision?: number) {
  const body: Record<string, unknown> = { by: "user", profile };
  if (typeof expectedRevision === "number") {
    body.expected_revision = Math.trunc(expectedRevision);
  }
  return apiJson<{ profile: ActorProfile }>(`/api/v1/actor_profiles`, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function deleteActorProfile(profileId: string, opts?: { forceDetach?: boolean }) {
  const params = new URLSearchParams();
  params.set("by", "user");
  if (opts?.forceDetach) params.set("force_detach", "true");
  return apiJson<{ deleted: boolean; profile_id: string; detached_count?: number; detached?: ActorProfileUsage[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}?${params.toString()}`,
    { method: "DELETE" }
  );
}

export async function fetchActorProfilePrivateEnvKeys(profileId: string) {
  return apiJson<{ profile_id: string; keys: string[]; masked_values?: Record<string, string> }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/env_private?by=user`
  );
}

export async function updateActorProfilePrivateEnv(
  profileId: string,
  setVars: Record<string, string>,
  unsetKeys: string[],
  clear: boolean
) {
  return apiJson<{ profile_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/env_private`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", set: setVars, unset: unsetKeys, clear }),
    }
  );
}

export async function copyActorPrivateEnvToProfile(profileId: string, groupId: string, actorId: string) {
  return apiJson<{ profile_id: string; group_id: string; actor_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/copy_actor_secrets`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", group_id: groupId, actor_id: actorId }),
    }
  );
}

export async function copyActorProfilePrivateEnvFromProfile(profileId: string, sourceProfileId: string) {
  return apiJson<{ profile_id: string; source_profile_id: string; keys: string[] }>(
    `/api/v1/actor_profiles/${encodeURIComponent(profileId)}/copy_profile_secrets`,
    {
      method: "POST",
      body: JSON.stringify({ by: "user", source_profile_id: sourceProfileId }),
    }
  );
}

// ============ Context & Settings ============

export async function fetchContext(groupId: string) {
  return apiJson<GroupContext>(`/api/v1/groups/${encodeURIComponent(groupId)}/context`);
}

export async function fetchTasks(groupId: string) {
  return apiJson<{ tasks: Task[] }>(`/api/v1/groups/${encodeURIComponent(groupId)}/tasks`);
}

export async function contextSync(
  groupId: string,
  ops: Array<Record<string, unknown>>,
  dryRun: boolean = false
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/context`, {
    method: "POST",
    body: JSON.stringify({ ops, by: "user", dry_run: dryRun }),
  });
}

export async function updateVision(groupId: string, vision: string) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/context`, {
    method: "POST",
    body: JSON.stringify({ ops: [{ op: "vision.update", vision }], by: "user" }),
  });
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
  }>(`/api/v1/capabilities/allowlist`, {
    method: "PUT",
    body: JSON.stringify(body),
  });
}

export async function blockCapabilityGlobal(groupId: string, capabilityId: string, blocked: boolean, reason: string = "") {
  return apiJson<Record<string, unknown>>(`/api/v1/capabilities/block`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      actor_id: "user",
      group_id: groupId,
      capability_id: capabilityId,
      blocked,
      reason,
      scope: "global",
    }),
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
  replyRequired = false
) {
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("to_json", JSON.stringify(to));
    form.append("path", "");
    form.append("priority", priority);
    form.append("reply_required", replyRequired ? "true" : "false");
    for (const f of files) form.append("files", f);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/send_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
    method: "POST",
    body: JSON.stringify({ text, by: "user", to, path: "", priority, reply_required: replyRequired }),
  });
}

export async function replyMessage(
  groupId: string,
  text: string,
  to: string[],
  replyTo: string,
  files?: File[],
  priority: "normal" | "attention" = "normal",
  replyRequired = false
) {
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("to_json", JSON.stringify(to));
    form.append("reply_to", replyTo);
    form.append("priority", priority);
    form.append("reply_required", replyRequired ? "true" : "false");
    for (const f of files) form.append("files", f);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/reply_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/reply`, {
    method: "POST",
    body: JSON.stringify({ text, by: "user", to, reply_to: replyTo, priority, reply_required: replyRequired }),
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

export async function ackMessage(groupId: string, eventId: string) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/events/${encodeURIComponent(eventId)}/ack`, {
    method: "POST",
    body: JSON.stringify({ by: "user" }),
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

export async function updateRemoteAccessConfig(args: {
  provider?: "off" | "manual" | "tailscale";
  mode?: string;
  enforceWebToken?: boolean;
  webHost?: string;
  webPort?: number;
  webPublicUrl?: string;
  webToken?: string;
  clearWebToken?: boolean;
}) {
  return apiJson<{ remote_access: RemoteAccessState }>("/api/v1/remote_access", {
    method: "PUT",
    body: JSON.stringify({
      by: "user",
      provider: args.provider,
      mode: args.mode,
      enforce_web_token: args.enforceWebToken,
      web_host: args.webHost,
      web_port: args.webPort,
      web_public_url: args.webPublicUrl,
      web_token: args.webToken,
      clear_web_token: args.clearWebToken,
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

export async function fetchGroupSpaceStatus(groupId: string, provider: string = "notebooklm") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/status?provider=${encodeURIComponent(provider)}`
  );
}

export async function bindGroupSpace(groupId: string, remoteSpaceId: string = "", provider: string = "notebooklm") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/bind`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        provider,
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
    binding?: Record<string, unknown>;
    spaces: GroupSpaceRemoteSpace[];
  }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/spaces?provider=${encodeURIComponent(provider)}`
  );
}

export async function unbindGroupSpace(groupId: string, provider: string = "notebooklm") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/bind`,
    {
      method: "POST",
      body: JSON.stringify({
        by: "user",
        provider,
        action: "unbind",
        remote_space_id: "",
      }),
    }
  );
}

export async function ingestGroupSpace(args: {
  groupId: string;
  provider?: string;
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
      kind: args.kind,
      payload: args.payload || {},
      idempotency_key: String(args.idempotencyKey || ""),
    }),
  });
}

export async function queryGroupSpace(args: {
  groupId: string;
  provider?: string;
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
      query: args.query,
      options: args.options || {},
    }),
  });
}

export async function fetchGroupSpaceSources(groupId: string, provider: string = "notebooklm") {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    binding?: Record<string, unknown>;
    action: "list";
    sources: GroupSpaceSource[];
    list_result?: Record<string, unknown>;
  }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/sources?provider=${encodeURIComponent(provider)}`
  );
}

export async function actionGroupSpaceSource(args: {
  groupId: string;
  provider?: string;
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
      action: args.action,
      source_id: args.sourceId,
      new_title: String(args.newTitle || ""),
    }),
  });
}

export async function fetchGroupSpaceArtifacts(
  groupId: string,
  provider: string = "notebooklm",
  kind: string = ""
) {
  const params = new URLSearchParams({
    provider: String(provider || "notebooklm"),
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
  state?: string;
  limit?: number;
}) {
  const params = new URLSearchParams({
    provider: String(args.provider || "notebooklm"),
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
      action: args.action,
      job_id: args.jobId,
    }),
  });
}

export async function syncGroupSpace(args: {
  groupId: string;
  provider?: string;
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
  action: "status" | "start" | "cancel";
  timeoutSeconds?: number;
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
    }),
  });
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
