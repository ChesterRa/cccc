import type { WorkspaceEntry, WorkspaceFile, WorkspaceListing } from "../../types";
import { apiJson, asRecord, withAuthToken, type ApiResponse } from "./base";

function groupPath(groupId: string, suffix: string): string {
  return `/api/v1/groups/${encodeURIComponent(groupId)}/workspace/${suffix}`;
}

/** Native media requests use the existing Web session and Connect frame authority. */
export function workspaceContentUrl(
  groupId: string,
  file: Pick<WorkspaceFile, "path" | "scope_key" | "scope_url">,
  download = false,
): string {
  const query = new URLSearchParams({
    path: file.path,
    scope_key: file.scope_key,
    scope_url: file.scope_url,
  });
  if (download) query.set("download", "true");
  return withAuthToken(`${groupPath(groupId, "content")}?${query}`);
}

function invalidResponse<T>(message: string): ApiResponse<T> {
  return { ok: false, error: { code: "invalid_response", message } };
}

function isEntry(value: unknown): value is WorkspaceEntry {
  const item = asRecord(value);
  return (
    !!item &&
    typeof item.name === "string" &&
    typeof item.path === "string" &&
    typeof item.is_dir === "boolean" &&
    (item.is_symlink === undefined || typeof item.is_symlink === "boolean") &&
    (item.unavailable === undefined ||
      (typeof item.unavailable === "string" &&
        ["missing", "outside_scope", "unreadable", "unsupported"].includes(item.unavailable)))
  );
}

/** Resolve pasted paths without reading file contents or enumerating a directory. */
export async function resolveWorkspacePath(
  groupId: string,
  path: string,
  scopeKey: string,
  scopeUrl: string,
): Promise<ApiResponse<{ path: string; is_dir: boolean }>> {
  const response = await apiJson<unknown>(
    `${groupPath(groupId, "path")}?${new URLSearchParams({ path, scope_key: scopeKey, scope_url: scopeUrl })}`,
  );
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (
    !result ||
    result.scope_key !== scopeKey ||
    result.scope_url !== scopeUrl ||
    typeof result.path !== "string" ||
    typeof result.is_dir !== "boolean"
  ) {
    return invalidResponse("Invalid workspace path response");
  }
  return { ok: true, result: { path: result.path, is_dir: result.is_dir } };
}

export async function fetchWorkspaceListing(
  groupId: string,
  path: string,
  options: { scopeKey: string; scopeUrl: string; showIgnored?: boolean },
): Promise<ApiResponse<WorkspaceListing>> {
  const query = new URLSearchParams({ scope_key: options.scopeKey, scope_url: options.scopeUrl });
  if (path) query.set("path", path);
  // Axum deserializes this into a Rust bool, which only accepts "true"/"false".
  if (options?.showIgnored) query.set("show_ignored", "true");
  const suffix = query.size ? `list?${query}` : "list";
  const response = await apiJson<unknown>(groupPath(groupId, suffix));
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (
    !result ||
    result.scope_key !== options.scopeKey ||
    result.scope_url !== options.scopeUrl ||
    typeof result.path !== "string" ||
    !(result.parent === null || typeof result.parent === "string") ||
    !Array.isArray(result.items) ||
    !result.items.every(isEntry)
  ) {
    return invalidResponse<WorkspaceListing>("Invalid workspace list response");
  }
  return {
    ok: true,
    result: {
      scope_key: options.scopeKey,
      scope_url: options.scopeUrl,
      root_path: typeof result.root_path === "string" ? result.root_path : "",
      path: result.path,
      parent: result.parent,
      items: result.items,
    },
  };
}

export async function fetchWorkspaceFile(
  groupId: string,
  path: string,
  scopeKey: string,
  scopeUrl: string,
): Promise<ApiResponse<WorkspaceFile>> {
  const response = await apiJson<unknown>(
    `${groupPath(groupId, "file")}?${new URLSearchParams({ path, scope_key: scopeKey, scope_url: scopeUrl })}`,
  );
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (
    !result ||
    result.scope_key !== scopeKey ||
    result.scope_url !== scopeUrl ||
    typeof result.path !== "string" ||
    typeof result.sha256 !== "string"
  ) {
    return invalidResponse<WorkspaceFile>("Invalid workspace file response");
  }
  return {
    ok: true,
    result: {
      scope_key: scopeKey,
      scope_url: scopeUrl,
      path: result.path,
      content: typeof result.content === "string" ? result.content : "",
      bytes: typeof result.bytes === "number" ? result.bytes : 0,
      mime_type: typeof result.mime_type === "string" ? result.mime_type : "",
      binary: result.binary === true,
      truncated: result.truncated === true,
      sha256: result.sha256,
    },
  };
}

/**
 * Saves into the opened workspace, echoing its identity and digest so the Web endpoint
 * can reject a scope change or a file changed on disk since it was read.
 */
export async function saveWorkspaceFile(
  groupId: string,
  path: string,
  content: string,
  sha256: string,
  scopeKey: string,
  scopeUrl: string,
): Promise<ApiResponse<{ path: string; sha256: string; created: boolean }>> {
  const response = await apiJson<unknown>(groupPath(groupId, "file"), {
    method: "PUT",
    body: JSON.stringify({ path, content, sha256, scope_key: scopeKey, scope_url: scopeUrl }),
  });
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (!result || typeof result.sha256 !== "string") {
    return invalidResponse<{ path: string; sha256: string; created: boolean }>(
      "Invalid workspace save response",
    );
  }
  return {
    ok: true,
    result: {
      path: typeof result.path === "string" ? result.path : path,
      sha256: result.sha256,
      created: result.created === true,
    },
  };
}

export type WorkspaceOperation =
  | { operation: "create"; path: string; directory: boolean }
  | { operation: "move"; path: string; destination: string }
  | { operation: "delete"; path: string };

export function changeWorkspaceEntry(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  operation: WorkspaceOperation,
): Promise<ApiResponse<{ path: string; destination?: string; mime_type?: string }>> {
  return apiJson(withAuthToken(groupPath(groupId, "entries")), {
    method: "POST",
    body: JSON.stringify({ ...operation, scope_key: scopeKey, scope_url: scopeUrl }),
  });
}

export function uploadWorkspaceFile(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  path: string,
  file: File,
  signal: AbortSignal,
): Promise<ApiResponse<{ path: string; bytes: number }>> {
  const query = new URLSearchParams({
    scope_key: scopeKey,
    scope_url: scopeUrl,
    path,
    bytes: String(file.size),
  });
  return apiJson(withAuthToken(`${groupPath(groupId, "upload")}?${query}`), {
    method: "POST",
    headers: { "content-type": "application/octet-stream" },
    body: file,
    signal,
  });
}

export type WorkspaceChange = {
  path: string;
  index: string;
  worktree: string;
  previous_path: string | null;
  untracked: boolean;
  conflicted: boolean;
  directory: boolean;
};
export type WorkspaceChanges = {
  repository: boolean;
  branch: string;
  entries: WorkspaceChange[];
  limited: boolean;
};
export type WorkspaceDiffSide = "worktree" | "staged";
export type WorkspacePatch = { patch: string; limited: boolean };
export function fetchWorkspaceChanges(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  signal: AbortSignal,
): Promise<ApiResponse<WorkspaceChanges>> {
  const query = new URLSearchParams({ scope_key: scopeKey, scope_url: scopeUrl });
  return apiJson(withAuthToken(`${groupPath(groupId, "changes")}?${query}`), { signal });
}
export function fetchWorkspaceDiff(
  groupId: string,
  scopeKey: string,
  scopeUrl: string,
  path: string,
  side: WorkspaceDiffSide,
  signal: AbortSignal,
): Promise<ApiResponse<WorkspacePatch>> {
  const query = new URLSearchParams({ scope_key: scopeKey, scope_url: scopeUrl, path, side });
  return apiJson(withAuthToken(`${groupPath(groupId, "diff")}?${query}`), { signal });
}
