import type { DirItem, DirSuggestion, RuntimeInfo } from "../../types";
import {
  apiJson,
  asRecord,
  pingRequestKey,
  RECENT_BOOTSTRAP_READ_TTL_MS,
  reuseRecentReadRequest,
  type ApiResponse,
} from "./base";

export async function fetchPing(options?: { includeHome?: boolean }) {
  const includeHome = Boolean(options?.includeHome);
  const suffix = includeHome ? "?include_home=1" : "";
  return reuseRecentReadRequest(pingRequestKey(includeHome), RECENT_BOOTSTRAP_READ_TTL_MS, () =>
    apiJson<{
      home?: string;
      daemon: unknown;
      version: string;
      web?: { mode?: string; read_only?: boolean };
    }>(`/api/v1/ping${suffix}`),
  );
}

export async function fetchRuntimes() {
  return apiJson<{ runtimes: RuntimeInfo[]; available: string[] }>("/api/v1/runtimes");
}

export async function fetchDirSuggestions() {
  const response = await apiJson<unknown>("/api/v1/fs/recent");
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (!Array.isArray(result?.suggestions)) {
    return invalidResponse<{ suggestions: DirSuggestion[] }>("Invalid filesystem recent response");
  }
  const suggestions = result.suggestions.filter(isDirSuggestion);
  if (suggestions.length !== result.suggestions.length) {
    return invalidResponse<{ suggestions: DirSuggestion[] }>("Invalid filesystem recent response");
  }
  return { ok: true as const, result: { suggestions } };
}

export async function fetchDirContents(path: string) {
  const response = await apiJson<unknown>(`/api/v1/fs/list?path=${encodeURIComponent(path)}`);
  if (!response.ok) return response;
  const result = asRecord(response.result);
  if (
    !result ||
    typeof result.path !== "string" ||
    !(result.parent === null || typeof result.parent === "string") ||
    !Array.isArray(result.items) ||
    !result.items.every(isDirItem)
  ) {
    return invalidResponse<{ path: string; parent: string | null; items: DirItem[] }>(
      "Invalid filesystem list response",
    );
  }
  return {
    ok: true as const,
    result: { path: result.path, parent: result.parent, items: result.items },
  };
}

export async function resolveScopeRoot(path: string) {
  return apiJson<{ path: string; scope_root: string; scope_key: string; git_remote: string }>(
    `/api/v1/fs/scope_root?path=${encodeURIComponent(path)}`,
  );
}

function invalidResponse<T>(message: string): ApiResponse<T> {
  return { ok: false, error: { code: "invalid_response", message } };
}

function isDirItem(value: unknown): value is DirItem {
  const item = asRecord(value);
  return (
    !!item &&
    typeof item.name === "string" &&
    typeof item.path === "string" &&
    typeof item.is_dir === "boolean"
  );
}

function isDirSuggestion(value: unknown): value is DirSuggestion {
  const item = asRecord(value);
  return (
    !!item &&
    typeof item.name === "string" &&
    typeof item.path === "string" &&
    typeof item.icon === "string"
  );
}
