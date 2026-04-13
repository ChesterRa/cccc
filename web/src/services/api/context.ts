import type {
  AutomationRuleSet,
  CapabilityBlockEntry,
  CapabilityImportRecord,
  CapabilityOverviewItem,
  CapabilitySourceState,
  CapabilityStateResult,
  ContextDetailLevel,
  CoordinationBrief,
  GroupLearningSnapshot,
  LearningProceduralSkill,
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

export async function fetchGroupLearning(groupId: string, init?: RequestInit) {
  return apiJson<GroupLearningSnapshot>(`/api/v1/groups/${encodeURIComponent(groupId)}/learning`, init);
}

export type ProceduralSkillPayload = {
  skill_id?: string;
  title: string;
  goal: string;
  steps: string[];
  constraints?: string[];
  failure_signals?: string[];
  stability?: string;
  review_mode?: string;
  status?: string;
  source_experience_candidate_id?: string;
};

export async function createProceduralSkill(groupId: string, payload: ProceduralSkillPayload) {
  return apiJson<{ skill: LearningProceduralSkill }>(`/api/v1/groups/${encodeURIComponent(groupId)}/learning/skills`, {
    method: "POST",
    body: JSON.stringify(payload),
  });
}

export async function updateProceduralSkill(groupId: string, skillId: string, payload: ProceduralSkillPayload) {
  return apiJson<{ skill: LearningProceduralSkill }>(`/api/v1/groups/${encodeURIComponent(groupId)}/learning/skills/${encodeURIComponent(skillId)}`, {
    method: "PUT",
    body: JSON.stringify(payload),
  });
}

export async function deleteProceduralSkill(groupId: string, skillId: string) {
  return apiJson<{ skill_id: string }>(`/api/v1/groups/${encodeURIComponent(groupId)}/learning/skills/${encodeURIComponent(skillId)}`, {
    method: "DELETE",
  });
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

export async function fetchCapabilityOverview(opts?: { query?: string; limit?: number; includeIndexed?: boolean }) {
  const params = new URLSearchParams();
  if (String(opts?.query || "").trim()) params.set("query", String(opts?.query || "").trim());
  if (typeof opts?.limit === "number" && Number.isFinite(opts.limit)) {
    params.set("limit", String(Math.max(1, Math.trunc(opts.limit))));
  }
  if (typeof opts?.includeIndexed === "boolean") {
    params.set("include_indexed", opts.includeIndexed ? "true" : "false");
  }
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
  opts?: { capabilityId?: string; noCache?: boolean; signal?: AbortSignal },
) {
  const params = new URLSearchParams();
  if (actorId) params.set("actor_id", actorId);
  if (String(opts?.capabilityId || "").trim()) params.set("capability_id", String(opts?.capabilityId || "").trim());
  if (opts?.noCache) params.set("_", String(Date.now()));
  const suffix = params.toString() ? `?${params.toString()}` : "";
  return apiJson<CapabilityStateResult>(`/api/v1/groups/${encodeURIComponent(groupId)}/capabilities/state${suffix}`, {
    signal: opts?.signal,
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
