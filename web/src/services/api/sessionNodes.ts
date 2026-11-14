import { apiJson } from "./base";

export interface SessionNodeDevice {
  node_id: string;
  name: string;
  enabled: boolean;
  connected: boolean;
}

export const fetchSessionNodes = () =>
  apiJson<{ supported: boolean; devices: SessionNodeDevice[] }>("/api/v1/session_nodes");

export const createSessionNodeInvitation = () =>
  apiJson<{ ticket: string; expires_at: string }>("/api/v1/session_nodes/invitations", {
    method: "POST",
  });

export const revokeSessionNode = (nodeId: string) =>
  apiJson<{ revoked: boolean }>(`/api/v1/session_nodes/${encodeURIComponent(nodeId)}/revoke`, {
    method: "POST",
  });
