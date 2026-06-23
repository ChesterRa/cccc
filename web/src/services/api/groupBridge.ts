import { apiJson } from "./base";

// Group Bridge remote-send client.

export interface GroupBridgeRegistration {
  registration_id: string;
  group_id: string;
  url: string;
  transport: string;
  remote_group_id: string;
  remote_peer_id?: string;
  credential_ref: string;
  user_id: string;
  status: string;
  created_at: string;
  updated_at: string;
  last_sync_at?: string | null;
  last_error?: string | null;
}

export interface GroupBridgeDeliveryError {
  code: string;
  message: string;
  retriable?: boolean;
  transport?: string;
  http_status?: number | null;
}

export interface GroupBridgeDeliveryReceipt {
  status: string;
  ok?: boolean;
  registration_id?: string;
  idempotency_key?: string;
  remote_event_id?: string | null;
  transport?: string;
  error?: GroupBridgeDeliveryError | null;
}

export interface GroupBridgeIdentity {
  node_id: string;
  peer_id: string;
}

export type GroupBridgeAccessLevel = "messages" | "read" | "full";

export interface GroupBridgePairingInvite {
  invite_id: string;
  group_id: string;
  remote_group_id: string;
  remote_peer_id: string;
  transport: string;
  status: string;
  expires_at: string;
  pairing_code?: string;
  request_id?: string;
}

export interface GroupBridgePairingRequest {
  request_id: string;
  invite_id: string;
  group_id: string;
  remote_group_id: string;
  remote_peer_id: string;
  status: string;
  registration_id?: string;
  approved_by?: string;
  rejected_by?: string;
  rejection_reason?: string;
}

export interface GroupBridgeTrust {
  trust_id: string;
  request_id: string;
  registration_id: string;
  group_id: string;
  remote_group_id: string;
  remote_group_title?: string;
  remote_endpoint?: string;
  remote_peer_id: string;
  transport: string;
  status: string;
  access_level?: GroupBridgeAccessLevel | string;
  remote_access_level?: GroupBridgeAccessLevel | string;
  access_updated_by?: string;
}

export interface GroupBridgePairingOutbound {
  outbound_id: string;
  local_group_id: string;
  issuer_endpoint: string;
  issuer_group_id: string;
  issuer_group_title?: string;
  issuer_peer_id: string;
  invite_id: string;
  status: string;
  remote_request?: Record<string, unknown>;
  last_error?: string;
  created_at?: string;
  updated_at?: string;
}

export interface PairingInviteInput {
  groupId: string;
  remoteGroupId?: string;
  remotePeerId?: string;
  ttlSeconds?: number;
}

export interface PairingRequestInput {
  pairingCode: string;
  requesterGroupId: string;
  requesterPeerId: string;
  inviteId?: string;
}

export interface RemotePairingRequestInput {
  localGroupId: string;
  localGroupTitle?: string;
  payload: Record<string, unknown>;
}

export interface PairingConnectionInfoInput {
  groupId: string;
  inviteId: string;
  issuerEndpoint: string;
  issuerGroupTitle?: string;
}

export async function fetchGroupBridgeStatus(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ registrations: GroupBridgeRegistration[] }>(`/api/group-bridge/status${suffix}`);
}

export async function unregisterGroupBridge(registrationId: string) {
  return apiJson<{ deleted: boolean }>("/api/group-bridge/unregister", {
    method: "POST",
    body: JSON.stringify({ registration_id: registrationId }),
  });
}

export async function fetchGroupBridgeDeliveryStatus(registrationId: string, idempotencyKey: string) {
  return apiJson<{ receipt: GroupBridgeDeliveryReceipt | null }>(
    `/api/group-bridge/registrations/${encodeURIComponent(registrationId)}/deliveries/${encodeURIComponent(idempotencyKey)}`,
  );
}

export async function fetchGroupBridgeIdentity() {
  return apiJson<{ identity: GroupBridgeIdentity }>("/api/group-bridge/pairing/identity");
}

export async function createGroupBridgePairingInvite(input: PairingInviteInput) {
  return apiJson<{ invite: GroupBridgePairingInvite }>("/api/group-bridge/pairing/invites", {
    method: "POST",
    body: JSON.stringify({
      group_id: input.groupId,
      remote_group_id: input.remoteGroupId,
      remote_peer_id: input.remotePeerId,
      ttl_seconds: input.ttlSeconds ?? 600,
    }),
  });
}

export async function createGroupBridgePairingConnectionInfo(input: PairingConnectionInfoInput) {
  return apiJson<{ payload: Record<string, unknown> }>("/api/group-bridge/pairing/connection-info", {
    method: "POST",
    body: JSON.stringify({
      group_id: input.groupId,
      invite_id: input.inviteId,
      issuer_endpoint: input.issuerEndpoint,
      issuer_group_title: input.issuerGroupTitle ?? "",
    }),
  });
}

export async function createGroupBridgePairingRequest(input: PairingRequestInput) {
  const body: Record<string, unknown> = {
    pairing_code: input.pairingCode,
    requester_group_id: input.requesterGroupId,
    requester_peer_id: input.requesterPeerId,
  };
  if (input.inviteId) {
    body.invite_id = input.inviteId;
  }
  return apiJson<{ request: GroupBridgePairingRequest }>("/api/group-bridge/pairing/requests", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function createGroupBridgeRemotePairingRequest(input: RemotePairingRequestInput) {
  return apiJson<{ outbound: Record<string, unknown> }>("/api/group-bridge/pairing/remote-requests", {
    method: "POST",
    body: JSON.stringify({
      local_group_id: input.localGroupId,
      local_group_title: input.localGroupTitle ?? "",
      payload: input.payload,
    }),
  });
}

export async function fetchGroupBridgePairingRequests(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ requests: GroupBridgePairingRequest[] }>(`/api/group-bridge/pairing/requests${suffix}`);
}

export async function approveGroupBridgePairingRequest(requestId: string, approverUserId = "") {
  return apiJson<{ request: GroupBridgePairingRequest; registration: GroupBridgeRegistration; trust: GroupBridgeTrust | null }>(
    `/api/group-bridge/pairing/requests/${encodeURIComponent(requestId)}/approve`,
    {
      method: "POST",
      body: JSON.stringify({ approver_user_id: approverUserId }),
    },
  );
}

export async function rejectGroupBridgePairingRequest(requestId: string, rejectedBy = "", reason = "") {
  return apiJson<{ request: GroupBridgePairingRequest }>(
    `/api/group-bridge/pairing/requests/${encodeURIComponent(requestId)}/reject`,
    {
      method: "POST",
      body: JSON.stringify({ rejected_by: rejectedBy, reason }),
    },
  );
}

export async function fetchGroupBridgeTrusts(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ trusts: GroupBridgeTrust[] }>(`/api/group-bridge/pairing/trusts${suffix}`);
}

export async function revokeGroupBridgeTrust(trustId: string, revokedBy = "") {
  return apiJson<{ trust: GroupBridgeTrust }>(
    `/api/group-bridge/pairing/trusts/${encodeURIComponent(trustId)}/revoke`,
    {
      method: "POST",
      body: JSON.stringify({ revoked_by: revokedBy }),
    },
  );
}

export async function updateGroupBridgeTrustAccess(trustId: string, accessLevel: GroupBridgeAccessLevel, updatedBy = "") {
  return apiJson<{ trust: GroupBridgeTrust }>(
    `/api/group-bridge/pairing/trusts/${encodeURIComponent(trustId)}/access`,
    {
      method: "POST",
      body: JSON.stringify({ access_level: accessLevel, updated_by: updatedBy }),
    },
  );
}

export async function refreshGroupBridgeTrustRemoteInfo(trustId: string) {
  return apiJson<{ trust: GroupBridgeTrust; remote_status: Record<string, unknown> }>(
    `/api/group-bridge/pairing/trusts/${encodeURIComponent(trustId)}/refresh`,
    { method: "POST" },
  );
}

export async function fetchGroupBridgePairingOutbounds(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ outbounds: GroupBridgePairingOutbound[] }>(`/api/group-bridge/pairing/outbounds${suffix}`);
}

export async function syncGroupBridgePairingOutbound(outboundId: string) {
  return apiJson<{ outbound: GroupBridgePairingOutbound }>(
    `/api/group-bridge/pairing/outbounds/${encodeURIComponent(outboundId)}/sync`,
    { method: "POST" },
  );
}

export async function deleteGroupBridgePairingOutbound(outboundId: string) {
  return apiJson<{ deleted: boolean }>(
    `/api/group-bridge/pairing/outbounds/${encodeURIComponent(outboundId)}/delete`,
    { method: "POST" },
  );
}
