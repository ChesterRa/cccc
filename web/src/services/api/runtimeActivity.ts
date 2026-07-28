import type { RuntimeActivityEvent } from "../../types";
import { apiJson } from "./base";

export function fetchRuntimeActivitySnapshot(
  groupId: string,
  init?: RequestInit & { noCache?: boolean },
) {
  return apiJson<{ count: number; events: RuntimeActivityEvent[] }>(
    `/api/v1/groups/${encodeURIComponent(groupId)}/runtime-activity/snapshot`,
    init,
  );
}

export function runtimeActivityStreamPath(groupId: string, replay = true): string {
  const params = replay ? "" : "?replay=false";
  return `/api/v1/groups/${encodeURIComponent(groupId)}/runtime-activity/stream${params}`;
}
