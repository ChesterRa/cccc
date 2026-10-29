import type { Actor, GroupDoc, LocalSessionRecovery, LocalSessionSource } from "../../types";
import { apiJson, clearActorsReadOnlyRequest, clearGroupsReadRequest } from "./base";
export const fetchLocalSessions = () =>
  apiJson<{ sources: LocalSessionSource[] }>("/api/v1/local_sessions");

export const fetchLocalSessionRecovery = (groupId: string, actorId: string) =>
  apiJson<LocalSessionRecovery>(
    `/api/v1/local_sessions/recovery/${encodeURIComponent(groupId)}/${encodeURIComponent(actorId)}`,
  );

export const refreshLocalSessionRecovery = (groupId: string, actorId: string) =>
  apiJson<LocalSessionRecovery>(
    `/api/v1/local_sessions/recovery/${encodeURIComponent(groupId)}/${encodeURIComponent(actorId)}/refresh`,
    { method: "POST" },
  );

export const createLocalConversation = (sourceIds: string[]) => {
  clearGroupsReadRequest();
  return apiJson<{ group_id: string; group: GroupDoc; reused: boolean }>(
    "/api/v1/local_sessions/conversations",
    { method: "POST", body: JSON.stringify({ source_ids: sourceIds, direct: false }) },
  );
};

export const joinLocalSessions = async (groupId: string, sourceIds: string[]) => {
  clearGroupsReadRequest();
  clearActorsReadOnlyRequest(groupId);
  try {
    return await apiJson<{ group_id: string; group: GroupDoc; reused: boolean }>(
      "/api/v1/local_sessions/join",
      { method: "POST", body: JSON.stringify({ group_id: groupId, source_ids: sourceIds }) },
    );
  } finally {
    clearGroupsReadRequest();
    clearActorsReadOnlyRequest(groupId);
  }
};

export const resumeLocalSessionAccess = async (
  groupId: string,
  actorId: string,
  sourceId: string,
) => {
  clearActorsReadOnlyRequest(groupId);
  clearGroupsReadRequest();
  try {
    return await apiJson<{ actor: Actor }>(
      `/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}`,
      {
        method: "POST",
        body: JSON.stringify({ by: "user", enabled: true, external_session_id: sourceId }),
      },
    );
  } finally {
    clearActorsReadOnlyRequest(groupId);
    clearGroupsReadRequest();
  }
};
