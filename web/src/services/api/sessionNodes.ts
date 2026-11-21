import { apiJson } from "./base";

export interface SessionNodeDevice {
  node_id: string;
  name: string;
  enabled: boolean;
  connected: boolean;
}

export interface SessionNodeJoinRequest {
  request_id: string;
  name: string;
  node_id: string;
  user_code: string;
  expires_at: string;
}

export type SessionNodeJoinStatus = "pending" | "approved" | "denied" | "expired";
export type SessionNodeJoinDetail = SessionNodeJoinRequest & { status: SessionNodeJoinStatus };

export const fetchSessionNodes = (signal?: AbortSignal) =>
  apiJson<{
    supported: boolean;
    devices: SessionNodeDevice[];
    join_supported?: boolean;
    join_url?: string | null;
  }>("/api/v1/session_nodes", { signal, cache: "no-store" });

export const fetchPendingSessionNodeJoins = (signal: AbortSignal) =>
  apiJson<{ requests: SessionNodeJoinRequest[] }>("/api/v1/session_nodes/join/pending", {
    signal,
    cache: "no-store",
  });

const joinRequestPath = (id: string) =>
  `/api/v1/session_nodes/join/requests/${encodeURIComponent(id)}`;

export const fetchSessionNodeJoin = (id: string, signal: AbortSignal) =>
  apiJson<{ request: SessionNodeJoinDetail }>(joinRequestPath(id), { signal, cache: "no-store" });

export const decideSessionNodeJoin = (id: string, decision: "approve" | "deny") =>
  apiJson<{ status: "approved" | "denied" }>(`${joinRequestPath(id)}/${decision}`, {
    method: "POST",
  });

export const createSessionNodeInvitation = () =>
  apiJson<{ ticket: string; expires_at: string }>("/api/v1/session_nodes/invitations", {
    method: "POST",
  });

export const revokeSessionNode = (nodeId: string) =>
  apiJson<{ revoked: boolean }>(`/api/v1/session_nodes/${encodeURIComponent(nodeId)}/revoke`, {
    method: "POST",
  });
