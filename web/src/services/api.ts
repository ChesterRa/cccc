// API service layer (centralized API calls).
import type {
  GroupMeta,
  GroupDoc,
  LedgerEvent,
  Actor,
  RuntimeInfo,
  GroupContext,
  GroupSettings,
  Task,
  DirItem,
  DirSuggestion,
  IMConfig,
  IMStatus,
  IMPlatform,
} from "../types";

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
        ...(init?.headers || {}),
      },
    });
  } catch (e) {
    // Network error
    return makeErrorResponse("NETWORK_ERROR", e instanceof Error ? e.message : "Network request failed");
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
        ...(init?.headers || {}),
      },
    });
  } catch (e) {
    return makeErrorResponse("NETWORK_ERROR", e instanceof Error ? e.message : "Network request failed");
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

export async function attachScope(
  groupId: string,
  path: string,
  options?: { createWorktree?: boolean; worktreeBranch?: string; baseBranch?: string }
) {
  const body: Record<string, unknown> = { path, by: "user" };
  if (options?.createWorktree) {
    body.create_worktree = true;
    if (options.baseBranch) {
      // Auto-branch mode: system creates cccc/<group_id> branch
      body.base_branch = options.baseBranch;
    } else if (options.worktreeBranch) {
      // Legacy mode: checkout existing branch (only if provided)
      body.worktree_branch = options.worktreeBranch;
    }
    // If neither is provided, let backend handle validation error
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/attach`, {
    method: "POST",
    body: JSON.stringify(body),
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
  return apiForm<{ template: unknown; diff: unknown; scope_root: string }>(
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
  });
  if (opts?.before) params.set("before", opts.before);
  if (opts?.after) params.set("after", opts.after);
  return apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`
  );
}

// ============ Actors ============

export async function fetchActors(groupId: string) {
  return apiJson<{ actors: Actor[] }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors?include_unread=true`
  );
}

export async function addActor(
  groupId: string,
  actorId: string,
  role: "peer" | "foreman",
  runtime: string,
  command: string
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/actors`, {
    method: "POST",
    body: JSON.stringify({
      actor_id: actorId,
      role,
      runner: "pty",
      runtime,
      command,
      env: {},
      default_scope_key: "",
      by: "user",
    }),
  });
}

export async function updateActor(
  groupId: string,
  actorId: string,
  runtime: string,
  command: string,
  title: string
) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
    {
      method: "POST",
      body: JSON.stringify({
        runtime,
        command: command.trim(),
        title: title.trim(),
        by: "user",
      }),
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

export async function updateSketch(groupId: string, sketch: string) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/context`, {
    method: "POST",
    body: JSON.stringify({ ops: [{ op: "sketch.update", sketch }], by: "user" }),
  });
}

export async function fetchSettings(groupId: string) {
  return apiJson<{ settings: GroupSettings }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/settings`
  );
}

export async function updateSettings(groupId: string, settings: Partial<GroupSettings>) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/settings`, {
    method: "PUT",
    body: JSON.stringify({ ...settings, by: "user" }),
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
  priority: "normal" | "attention" = "normal"
) {
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("to_json", JSON.stringify(to));
    form.append("path", "");
    form.append("priority", priority);
    for (const f of files) form.append("files", f);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/send_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
    method: "POST",
    body: JSON.stringify({ text, by: "user", to, path: "", priority }),
  });
}

export async function replyMessage(
  groupId: string,
  text: string,
  to: string[],
  replyTo: string,
  files?: File[],
  priority: "normal" | "attention" = "normal"
) {
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("to_json", JSON.stringify(to));
    form.append("reply_to", replyTo);
    form.append("priority", priority);
    for (const f of files) form.append("files", f);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/reply_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/reply`, {
    method: "POST",
    body: JSON.stringify({ text, by: "user", to, reply_to: replyTo, priority }),
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
  priority: "normal" | "attention" = "normal"
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(srcGroupId)}/send_cross_group`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      dst_group_id: dstGroupId,
      to,
      priority,
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

// ============ Git ============

export interface BranchInfo {
  name: string;
  is_remote: boolean;
  in_use: boolean;
  worktree_path: string | null;
  is_current: boolean;
}

export async function fetchBranches(repoPath: string, includeRemote: boolean = true) {
  return apiJson<{ branches: BranchInfo[] }>(
    `/api/v1/git/branches?repo_path=${encodeURIComponent(repoPath)}&include_remote=${includeRemote}`
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
