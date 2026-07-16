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
  return fetchContext(gid, { detail: opts?.detail ?? "summary", fresh: opts?.fresh });
}

export function openContextModalData(
  fetchContext: ContextModalFetch,
  groupId: string,
): Promise<void> {
  return loadContextModalDetail(fetchContext, groupId, { detail: "full" });
}

export function syncContextModalData(
  fetchContext: ContextModalFetch,
  groupId: string,
): Promise<void> {
  return loadContextModalDetail(fetchContext, groupId, { detail: "full" });
}

export function reloadContextModalData(
  fetchContext: ContextModalFetch,
  groupId: string,
): Promise<void> {
  return loadContextModalDetail(fetchContext, groupId, { detail: "full", fresh: true });
}
