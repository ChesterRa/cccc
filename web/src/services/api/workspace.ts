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
    typeof item.is_dir === "boolean"
  );
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
