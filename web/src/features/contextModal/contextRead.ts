import type { ContextDetailLevel } from "../../types";

export type ContextModalFetch = (
  groupId: string,
  opts?: { fresh?: boolean; detail?: ContextDetailLevel },
) => Promise<void>;

function loadContextModalDetail(
  fetchContext: ContextModalFetch,
  groupId: string,
  opts?: { fresh?: boolean; detail?: ContextDetailLevel },
): Promise<void> {
  const gid = String(groupId || "").trim();
  if (!gid) {
    return Promise.resolve();
  }
  return fetchContext(gid, { detail: opts?.detail ?? "overview", fresh: opts?.fresh });
}

export function openContextModalData(
  fetchContext: ContextModalFetch,
  groupId: string,
): Promise<void> {
  return loadContextModalDetail(fetchContext, groupId, { detail: "overview" });
}

export function syncContextModalData(
  fetchContext: ContextModalFetch,
  groupId: string,
): Promise<void> {
  return loadContextModalDetail(fetchContext, groupId, { detail: "overview" });
}

export function reloadContextModalData(
  fetchContext: ContextModalFetch,
  groupId: string,
): Promise<void> {
  return loadContextModalDetail(fetchContext, groupId, { detail: "overview", fresh: true });
}
