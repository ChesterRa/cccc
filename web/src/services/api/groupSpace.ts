import type {
  GroupSpaceArtifact,
  GroupSpaceJob,
  GroupSpaceProviderAuthStatus,
  GroupSpaceProviderCredentialStatus,
  GroupSpaceRemoteSpace,
  GroupSpaceSource,
  GroupSpaceStatus,
} from "../../types";
import {
  apiJson,
  ApiResponse,
  normalizePresentationBrowserSurfaceState,
  withAuthToken,
} from "./base";

export async function fetchGroupSpaceStatus(groupId: string, provider: string = "notebooklm") {
  return apiJson<GroupSpaceStatus>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/status?provider=${encodeURIComponent(provider)}`,
  );
}

export async function bindGroupSpace(
  groupId: string,
  remoteSpaceId: string = "",
  provider: string = "notebooklm",
  lane: "work" | "memory",
) {
  return apiJson<GroupSpaceStatus>(`/api/v1/groups/${encodeURIComponent(groupId)}/space/bind`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider,
      lane,
      action: "bind",
      remote_space_id: String(remoteSpaceId || ""),
    }),
  });
}

export async function fetchGroupSpaceSpaces(groupId: string, provider: string = "notebooklm") {
  return apiJson<{
    group_id: string;
    provider: string;
    provider_state?: Record<string, unknown>;
    bindings?: Record<string, Record<string, unknown>>;
    spaces: GroupSpaceRemoteSpace[];
  }>(`/api/v1/groups/${encodeURIComponent(groupId)}/space/spaces?provider=${encodeURIComponent(provider)}`);
}

export async function unbindGroupSpace(groupId: string, provider: string = "notebooklm", lane: "work" | "memory") {
  return apiJson<GroupSpaceStatus>(`/api/v1/groups/${encodeURIComponent(groupId)}/space/bind`, {
    method: "POST",
    body: JSON.stringify({
      by: "user",
      provider,
      lane,
      action: "unbind",
      remote_space_id: "",
    }),
  });
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
    completed: boolean;
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

export async function fetchGroupSpaceSources(
  groupId: string,
  provider: string = "notebooklm",
  lane: "work" | "memory",
  opts?: { fresh?: boolean },
) {
  const params = new URLSearchParams({
    provider: String(provider || "notebooklm"),
    lane: String(lane),
  });
  if (opts?.fresh) {
    params.set("fresh", "true");
  }
  return apiJson<{
    group_id: string;
    provider: string;
    provider_mode: string;
    binding?: Record<string, unknown>;
    action: "list";
    sources: GroupSpaceSource[];
    list_result?: Record<string, unknown>;
  }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/space/sources?${params.toString()}`,
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
  kind: string = "",
  opts?: { fresh?: boolean },
) {
  const params = new URLSearchParams({
    provider: String(provider || "notebooklm"),
    lane: String(lane),
  });
  if (String(kind || "").trim()) {
    params.set("kind", String(kind || "").trim().toLowerCase());
  }
  if (opts?.fresh) {
    params.set("fresh", "true");
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
    accepted?: boolean;
    completed?: boolean;
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
    `/api/v1/space/providers/${encodeURIComponent(provider)}/credential?by=user`,
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
    },
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
): Promise<ApiResponse<{ provider: string; browser_surface: ReturnType<typeof normalizePresentationBrowserSurfaceState> }>> {
  const resp = await controlGroupSpaceProviderAuth({ provider, action: "status" });
  if (!resp.ok) {
    return resp as ApiResponse<{ provider: string; browser_surface: ReturnType<typeof normalizePresentationBrowserSurfaceState> }>;
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
