import type { DeviceSessionSource, LocalSessionSource } from "../../types";
import { apiJson } from "./base";

export interface SessionNodeDevice {
  node_id: string;
  name: string;
  enabled: boolean;
  connected: boolean;
}

export interface SessionNodeEndpoint {
  endpoint_id: string;
  addresses: string[];
  relay_url?: string | null;
}

export interface SessionNodeRelayStatus {
  enabled: boolean;
  effective: boolean;
  public_url: string | null;
}

export interface SessionNodeStatus {
  supported: boolean;
  transport?: string;
  endpoint?: SessionNodeEndpoint | null;
  relay?: SessionNodeRelayStatus;
  devices: SessionNodeDevice[];
  supported_runtimes?: LocalSessionSource["runtime"][];
}

export const fetchSessionNodes = (signal?: AbortSignal) =>
  apiJson<SessionNodeStatus>("/api/v1/session_nodes", { signal, cache: "no-store" });

export const fetchDeviceSessionSources = (signal?: AbortSignal) =>
  apiJson<{ sources: DeviceSessionSource[] }>("/api/v1/session_nodes/sources", {
    signal,
    cache: "no-store",
  });

export const createSessionNodeInvitation = () =>
  apiJson<{ ticket: string; expires_at: string }>("/api/v1/session_nodes/invitations", {
    method: "POST",
  });

export const revokeSessionNode = (nodeId: string) =>
  apiJson<{ revoked: boolean }>(`/api/v1/session_nodes/${encodeURIComponent(nodeId)}/revoke`, {
    method: "POST",
  });

export const stopSessionNodeGateway = () =>
  apiJson<{ stopped: boolean }>("/api/v1/session_nodes/stop", { method: "POST" });

export const setSessionNodeRelay = (enabled: boolean) =>
  apiJson<{ relay: SessionNodeRelayStatus }>("/api/v1/session_nodes/relay", {
    method: "PUT",
    body: JSON.stringify({ enabled }),
  });
