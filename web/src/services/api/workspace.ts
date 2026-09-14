import type { WorkspaceEntry, WorkspaceFile, WorkspaceListing } from "../../types";
import { apiJson, asRecord, type ApiResponse } from "./base";

function groupPath(groupId: string, suffix: string): string {
  return `/api/v1/groups/${encodeURIComponent(groupId)}/workspace/${suffix}`;
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
  options?: { showIgnored?: boolean },
): Promise<ApiResponse<WorkspaceListing>> {
  const query = new URLSearchParams();
  if (path) query.set("path", path);
  // Axum deserializes this into a Rust bool, which only accepts "true"/"false".
  if (options?.showIgnored) query.set("show_ignored", "true");
  const suffix = query.size ? `list?${query}` : "list";
  const response = await apiJson<unknown>(groupPath(groupId, suffix));
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (
    !result ||
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
): Promise<ApiResponse<WorkspaceFile>> {
  const response = await apiJson<unknown>(
    `${groupPath(groupId, "file")}?path=${encodeURIComponent(path)}`,
  );
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (!result || typeof result.path !== "string" || typeof result.sha256 !== "string") {
    return invalidResponse<WorkspaceFile>("Invalid workspace file response");
  }
  return {
    ok: true,
    result: {
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
 * Saves `content`, echoing the `sha256` the file was read with so the daemon can refuse the
 * write when an Actor changed the same file in the meantime.
 */
export async function saveWorkspaceFile(
  groupId: string,
  path: string,
  content: string,
  sha256: string,
): Promise<ApiResponse<{ path: string; sha256: string; created: boolean }>> {
  const response = await apiJson<unknown>(groupPath(groupId, "file"), {
    method: "PUT",
    body: JSON.stringify({ path, content, sha256 }),
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
