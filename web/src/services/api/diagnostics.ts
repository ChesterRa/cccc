import { apiJson } from "./base";

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

export async function fetchDebugSnapshot(groupId: string) {
  return apiJson<Record<string, unknown>>(`/api/v1/debug/snapshot?group_id=${encodeURIComponent(groupId)}`);
}

export async function fetchTerminalTail(
  groupId: string,
  actorId: string,
  maxChars = 8000,
  stripAnsi = true,
  compact = true,
) {
  const params = new URLSearchParams({
    actor_id: actorId,
    max_chars: String(maxChars),
    strip_ansi: String(stripAnsi),
    compact: String(compact),
  });
  return apiJson<{ text: string; hint: string }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/terminal/tail?${params.toString()}`,
  );
}

export interface TerminalHistoryPage {
  text: string;
  hint: string;
  start_cursor: number;
  end_cursor: number;
  has_more: boolean;
  cursor_expired: boolean;
}

export async function fetchTerminalHistory(
  groupId: string,
  actorId: string,
  opts?: {
    before?: number | string | null;
    limitBytes?: number;
    stripAnsi?: boolean;
    compact?: boolean;
  },
) {
  const params = new URLSearchParams({
    actor_id: actorId,
    limit_bytes: String(opts?.limitBytes || 64000),
    strip_ansi: String(opts?.stripAnsi ?? false),
    compact: String(opts?.compact ?? false),
  });
  const before = opts?.before;
  if (before !== null && before !== undefined && String(before).trim()) {
    params.set("before", String(before));
  }
  return apiJson<TerminalHistoryPage>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/terminal/history?${params.toString()}`,
  );
}

export async function clearTerminalTail(groupId: string, actorId: string) {
  return apiJson(
    `/api/v1/groups/${encodeURIComponent(groupId)}/terminal/clear?actor_id=${encodeURIComponent(actorId)}`,
    { method: "POST" },
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
