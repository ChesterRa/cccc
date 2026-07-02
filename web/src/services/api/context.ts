import type {
  AutomationRuleSet,
  CapabilityBlockEntry,
  CapabilityImportRecord,
  CapabilityOverviewItem,
  CapabilitySourceInstance,
  CapabilitySourceState,
  CapabilityStateResult,
  ContextDetailLevel,
  CoordinationBrief,
  GroupAutomation,
  GroupContext,
  GroupSettings,
  LedgerEvent,
  MessageRef,
  Task,
} from "../../types";
import {
  apiForm,
  apiJson,
  ApiResponse,
  capabilityStateRequestKey,
  clearContextRequest,
  contextRequestKey,
  FetchContextOptions,
  normalizeContext,
  normalizeTask,
  reuseSharedReadRequest,
} from "./base";

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
    if (opts?.noCache || opts?.signal) {
      return apiJson<unknown>(`/api/v1/groups/${encodeURIComponent(gid)}/context?${params.toString()}`, {
        signal: opts?.signal,
      }).then((resp) => {
        if (!resp.ok) return resp as ApiResponse<GroupContext>;
        return { ok: true, result: normalizeContext(resp.result) } as ApiResponse<GroupContext>;
      });
    }
    return reuseSharedReadRequest(contextRequestKey(gid, detail), async () => {
      const resp = await apiJson<unknown>(`/api/v1/groups/${encodeURIComponent(gid)}/context?${params.toString()}`);
      if (!resp.ok) return resp as ApiResponse<GroupContext>;
      return { ok: true, result: normalizeContext(resp.result) } as ApiResponse<GroupContext>;
    });
  }
  if (opts?.noCache || opts?.signal) {
    const suffix = params.toString();
    return apiJson<unknown>(
      `/api/v1/groups/${encodeURIComponent(gid)}/context${suffix ? `?${suffix}` : ""}`,
      { signal: opts?.signal },
    ).then((resp) => {
      if (!resp.ok) return resp as ApiResponse<GroupContext>;
      return { ok: true, result: normalizeContext(resp.result) } as ApiResponse<GroupContext>;
    });
  }
  return reuseSharedReadRequest(contextRequestKey(gid, detail), async () => {
    const suffix = params.toString();
    const resp = await apiJson<unknown>(
      `/api/v1/groups/${encodeURIComponent(gid)}/context${suffix ? `?${suffix}` : ""}`,
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

export async function contextSync(groupId: string, ops: Array<Record<string, unknown>>, dryRun: boolean = false) {
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
  taskId?: string | null,
) {
  const op: Record<string, unknown> = { op: "coordination.note.add", kind, summary: String(summary || "") };
  if (taskId) op.task_id = String(taskId || "");
  return contextSync(groupId, [op]);
}

export async function updateCoordinationTask(groupId: string, task: Task) {
  const resolvedTaskType =
    String(task.task_type || "").trim() || (String(task.parent_id || "").trim() ? "free" : "standard");
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
    task_type: resolvedTaskType,
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

export async function fetchSettings(groupId: string, init?: RequestInit & { noCache?: boolean }) {
  return apiJson<{ settings: GroupSettings }>(`/api/v1/groups/${encodeURIComponent(groupId)}/settings`, init);
}

export async function fetchCapabilityOverview(opts?: {
  query?: string;
  limit?: number;
  offset?: number;
  includeIndexed?: boolean;
  kind?: "all" | "pack" | "mcp" | "skill";
  policy?: "all" | "actionable" | "blocked" | "indexed";
  sourceId?: string;
  groupId?: string;
  includeSourceInstances?: boolean;
}) {
  const params = new URLSearchParams();
  if (String(opts?.query || "").trim()) params.set("query", String(opts?.query || "").trim());
  if (typeof opts?.limit === "number" && Number.isFinite(opts.limit)) {
    params.set("limit", String(Math.max(1, Math.trunc(opts.limit))));
  }
  if (typeof opts?.offset === "number" && Number.isFinite(opts.offset)) {
    params.set("offset", String(Math.max(0, Math.trunc(opts.offset))));
  }
  if (typeof opts?.includeIndexed === "boolean") {
    params.set("include_indexed", opts.includeIndexed ? "true" : "false");
  }
  if (typeof opts?.includeSourceInstances === "boolean") {
    params.set("include_source_instances", opts.includeSourceInstances ? "true" : "false");
  }
  if (String(opts?.kind || "").trim() && String(opts?.kind || "").trim() !== "all") {
    params.set("kind", String(opts?.kind || "").trim());
  }
  if (String(opts?.policy || "").trim() && String(opts?.policy || "").trim() !== "all") {
    params.set("policy", String(opts?.policy || "").trim());
  }
  if (String(opts?.sourceId || "").trim() && String(opts?.sourceId || "").trim() !== "all") {
    params.set("source_id", String(opts?.sourceId || "").trim());
  }
  if (String(opts?.groupId || "").trim()) {
    params.set("group_id", String(opts?.groupId || "").trim());
  }
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return apiJson<{
    items: CapabilityOverviewItem[];
    count: number;
    total_count: number;
    offset: number;
    limit: number;
    has_more: boolean;
    query?: string;
    kind_counts?: { skill?: number; mcp?: number; pack?: number };
    sources?: Record<string, CapabilitySourceState>;
    source_instances?: CapabilitySourceInstance[];
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
  if (String(args.expectedRevision || "").trim()) {
    body.expected_revision = String(args.expectedRevision || "").trim();
  }
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

export async function fetchGroupCapabilityState(
  groupId: string,
  actorId: string = "user",
  opts?: { capabilityId?: string; noCache?: boolean; signal?: AbortSignal; view?: string },
) {
  const gid = String(groupId || "").trim();
  const aid = String(actorId || "user").trim() || "user";
  const capabilityId = String(opts?.capabilityId || "").trim();
  const params = new URLSearchParams();
  params.set("actor_id", aid);
  if (capabilityId) params.set("capability_id", capabilityId);
  const view = String(opts?.view || "").trim();
  if (view) params.set("view", view);
  if (opts?.noCache) params.set("_", String(Date.now()));
  const load = () => {
    const suffix = params.toString() ? `?${params.toString()}` : "";
    return apiJson<CapabilityStateResult>(`/api/v1/groups/${encodeURIComponent(gid)}/capabilities/state${suffix}`, {
      signal: opts?.signal,
    });
  };
  if (opts?.signal) return load();
  if (opts?.noCache) {
    return reuseSharedReadRequest(`${capabilityStateRequestKey(gid, aid, capabilityId, view)}:no-cache`, load);
  }
  return reuseSharedReadRequest(capabilityStateRequestKey(gid, aid, capabilityId, view), load);
}

export async function fetchSlashCommandCapabilityState(
  groupId: string,
  actorId: string = "user",
  opts?: { noCache?: boolean; signal?: AbortSignal },
) {
  return fetchGroupCapabilityState(groupId, actorId, {
    ...opts,
    view: "slash_commands",
  });
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
  },
) {
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/enable`, {
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
  });
}

export async function updateGroupCapabilityVisibility(
  groupId: string,
  capabilityId: string,
  opts?: {
    hidden?: boolean;
    actorId?: string;
    reason?: string;
  },
) {
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/visibility`, {
    method: "POST",
    body: JSON.stringify({
      capability_id: capabilityId,
      hidden: opts?.hidden ?? true,
      actor_id: opts?.actorId || "user",
      reason: opts?.reason || "",
    }),
  });
}

export async function useGroupCapability(
  groupId: string,
  args: {
    capabilityId: string;
    actorId?: string;
    toolName?: string;
    toolArguments?: Record<string, unknown>;
    scope?: "session" | "actor" | "group";
    ttlSeconds?: number;
    reason?: string;
  },
) {
  const body: Record<string, unknown> = {
    capability_id: args.capabilityId,
    actor_id: args.actorId || "user",
    scope: args.scope || "session",
    ttl_seconds: args.ttlSeconds || 3600,
    reason: args.reason || "",
  };
  if (String(args.toolName || "").trim()) body.tool_name = String(args.toolName || "").trim();
  if (args.toolArguments) body.tool_arguments = args.toolArguments;
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/use`, {
    method: "POST",
    body: JSON.stringify(body),
  });
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
  },
) {
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/import`, {
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
  });
}

export async function installCapabilityTarget(
  groupId: string,
  target: string,
  opts?: {
    scope?: "session" | "actor" | "group";
    actorId?: string;
    ttlSeconds?: number;
    reason?: string;
  },
) {
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/install`, {
    method: "POST",
    body: JSON.stringify({
      target,
      scope: opts?.scope || "actor",
      actor_id: opts?.actorId || "user",
      ttl_seconds: opts?.ttlSeconds || 3600,
      reason: opts?.reason || "",
    }),
  });
}

export async function uninstallCapability(groupId: string, capabilityId: string, opts?: { actorId?: string; reason?: string }) {
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/uninstall`, {
    method: "POST",
    body: JSON.stringify({
      capability_id: capabilityId,
      actor_id: opts?.actorId || "user",
      reason: opts?.reason || "",
    }),
  });
}

export async function deleteCapabilitySource(groupId: string, sourceId: string, opts?: { actorId?: string; reason?: string; sourceInstanceKey?: string }) {
  return apiJson<Record<string, unknown>>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/sources/delete`, {
    method: "POST",
    body: JSON.stringify({
      source_id: sourceId,
      source_instance_key: opts?.sourceInstanceKey || "",
      actor_id: opts?.actorId || "user",
      reason: opts?.reason || "",
    }),
  });
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

export async function manageAutomation(
  groupId: string,
  actions: Array<Record<string, unknown>>,
  expectedVersion?: number,
) {
  const body: Record<string, unknown> = { actions, by: "user" };
  if (typeof expectedVersion === "number" && Number.isFinite(expectedVersion)) {
    body.expected_version = Math.trunc(expectedVersion);
  }
  return apiJson<GroupAutomation>(`/api/v1/groups/${encodeURIComponent(groupId)}/automation/manage`, {
    method: "POST",
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

export async function fetchInbox(groupId: string, actorId: string, limit = 200) {
  return apiJson<{ messages: LedgerEvent[] }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/inbox/${encodeURIComponent(actorId)}?by=user&limit=${limit}`,
  );
}

export async function markInboxRead(groupId: string, actorId: string, eventId: string) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/inbox/${encodeURIComponent(actorId)}/read`, {
    method: "POST",
    body: JSON.stringify({ event_id: eventId, by: "user" }),
  });
}

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
    for (const file of files) form.append("files", file);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/send_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/send`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      to,
      path: "",
      priority,
      reply_required: replyRequired,
      client_id: clientId,
      refs: refs || [],
    }),
  });
}

export async function dispatchSlashSkill(
  groupId: string,
  payload: {
    taskText: string;
    command: string;
    capabilityId: string;
    to: string[];
    priority?: "normal" | "attention";
    replyRequired?: boolean;
    clientId?: string;
    replyTo?: string;
    quoteText?: string;
  },
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/slash_skill_dispatch`, {
    method: "POST",
    body: JSON.stringify({
      task_text: payload.taskText,
      command: payload.command,
      capability_id: payload.capabilityId,
      to: payload.to || [],
      priority: payload.priority || "normal",
      reply_required: Boolean(payload.replyRequired),
      client_id: payload.clientId || "",
      reply_to: payload.replyTo || "",
      quote_text: payload.quoteText || "",
    }),
  });
}

export async function trackedSendMessage(
  groupId: string,
  payload: {
    title: string;
    text: string;
    to: string[];
    outcome?: string;
    checklist?: Array<{ text: string; status?: string }>;
    assignee?: string;
    waiting_on?: "none" | "user" | "actor" | "external" | string;
    handoff_to?: string;
    notes?: string;
    priority?: "normal" | "attention";
    reply_required?: boolean;
    idempotency_key?: string;
    refs?: MessageRef[];
  },
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/tracked_send`, {
    method: "POST",
    body: JSON.stringify({
      title: payload.title,
      text: payload.text,
      by: "user",
      to: payload.to || [],
      outcome: payload.outcome || "",
      checklist: payload.checklist || [],
      assignee: payload.assignee || "",
      waiting_on: payload.waiting_on || "actor",
      handoff_to: payload.handoff_to || "",
      notes: payload.notes || "",
      priority: payload.priority || "normal",
      reply_required: payload.reply_required !== false,
      idempotency_key: payload.idempotency_key || "",
      refs: payload.refs || [],
    }),
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
    for (const file of files) form.append("files", file);
    return apiForm(`/api/v1/groups/${encodeURIComponent(groupId)}/reply_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(groupId)}/reply`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      to,
      reply_to: replyTo,
      priority,
      reply_required: replyRequired,
      client_id: clientId,
      refs: refs || [],
    }),
  });
}

export async function relayMessage(
  dstGroupId: string,
  text: string,
  to: string[],
  src: { groupId: string; eventId: string },
  quoteText = "",
) {
  return apiJson(`/api/v1/groups/${encodeURIComponent(dstGroupId)}/send`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      to,
      quote_text: quoteText,
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
  replyRequired = false,
  files?: File[],
  options?: {
    replyTo?: string;
    quoteText?: string;
    clientId?: string;
    remoteReplyToEventId?: string;
  },
) {
  const replyMetadata = {
    ...(options?.replyTo ? { reply_to: options.replyTo } : {}),
    ...(options?.quoteText ? { quote_text: options.quoteText } : {}),
    ...(options?.clientId ? { client_id: options.clientId } : {}),
    ...(options?.remoteReplyToEventId ? { remote_reply_to_event_id: options.remoteReplyToEventId } : {}),
  };
  if (files && files.length > 0) {
    const form = new FormData();
    form.append("by", "user");
    form.append("text", text);
    form.append("dst_group_id", dstGroupId);
    form.append("to_json", JSON.stringify(to));
    form.append("priority", priority);
    form.append("reply_required", replyRequired ? "true" : "false");
    if (options?.replyTo) form.append("reply_to", options.replyTo);
    if (options?.quoteText) form.append("quote_text", options.quoteText);
    if (options?.clientId) form.append("client_id", options.clientId);
    if (options?.remoteReplyToEventId) form.append("remote_reply_to_event_id", options.remoteReplyToEventId);
    for (const file of files) form.append("files", file);
    return apiForm(`/api/v1/groups/${encodeURIComponent(srcGroupId)}/send_cross_group_upload`, form);
  }
  return apiJson(`/api/v1/groups/${encodeURIComponent(srcGroupId)}/send_cross_group`, {
    method: "POST",
    body: JSON.stringify({
      text,
      by: "user",
      dst_group_id: dstGroupId,
      to,
      priority,
      reply_required: replyRequired,
      ...replyMetadata,
    }),
  });
}
