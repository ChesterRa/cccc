import { apiJson } from "./base";

// Federation remote-send client.

export interface FederationRegistration {
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

export interface FederationDeliveryError {
  code: string;
  message: string;
  retriable?: boolean;
  transport?: string;
  http_status?: number | null;
}

export interface FederationDeliveryReceipt {
  status: string;
  ok?: boolean;
  registration_id?: string;
  idempotency_key?: string;
  remote_event_id?: string | null;
  transport?: string;
  error?: FederationDeliveryError | null;
}

export interface FederationIdentity {
  node_id: string;
  peer_id: string;
}

export type GroupBridgeAccessLevel = "messages" | "read" | "full";

export interface FederationPairingInvite {
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

export interface FederationPairingRequest {
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

export interface FederationTrust {
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
  access_updated_by?: string;
}

export interface FederationPairingOutbound {
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

export async function fetchFederationStatus(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ registrations: FederationRegistration[] }>(`/api/federation/status${suffix}`);
}

export async function unregisterFederation(registrationId: string) {
  return apiJson<{ deleted: boolean }>("/api/federation/unregister", {
    method: "POST",
    body: JSON.stringify({ registration_id: registrationId }),
  });
}

export async function fetchFederationDeliveryStatus(registrationId: string, idempotencyKey: string) {
  return apiJson<{ receipt: FederationDeliveryReceipt | null }>(
    `/api/federation/registrations/${encodeURIComponent(registrationId)}/deliveries/${encodeURIComponent(idempotencyKey)}`,
  );
}

export async function fetchFederationIdentity() {
  return apiJson<{ identity: FederationIdentity }>("/api/federation/pairing/identity");
}

export async function createFederationPairingInvite(input: PairingInviteInput) {
  return apiJson<{ invite: FederationPairingInvite }>("/api/federation/pairing/invites", {
    method: "POST",
    body: JSON.stringify({
      group_id: input.groupId,
      remote_group_id: input.remoteGroupId,
      remote_peer_id: input.remotePeerId,
      ttl_seconds: input.ttlSeconds ?? 600,
    }),
  });
}

export async function createFederationPairingConnectionInfo(input: PairingConnectionInfoInput) {
  return apiJson<{ payload: Record<string, unknown> }>("/api/federation/pairing/connection-info", {
    method: "POST",
    body: JSON.stringify({
      group_id: input.groupId,
      invite_id: input.inviteId,
      issuer_endpoint: input.issuerEndpoint,
      issuer_group_title: input.issuerGroupTitle ?? "",
    }),
  });
}

export async function createFederationPairingRequest(input: PairingRequestInput) {
  const body: Record<string, unknown> = {
    pairing_code: input.pairingCode,
    requester_group_id: input.requesterGroupId,
    requester_peer_id: input.requesterPeerId,
  };
  if (input.inviteId) {
    body.invite_id = input.inviteId;
  }
  return apiJson<{ request: FederationPairingRequest }>("/api/federation/pairing/requests", {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export async function createFederationRemotePairingRequest(input: RemotePairingRequestInput) {
  return apiJson<{ outbound: Record<string, unknown> }>("/api/federation/pairing/remote-requests", {
    method: "POST",
    body: JSON.stringify({
      local_group_id: input.localGroupId,
      local_group_title: input.localGroupTitle ?? "",
      payload: input.payload,
    }),
  });
}

export async function fetchFederationPairingRequests(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ requests: FederationPairingRequest[] }>(`/api/federation/pairing/requests${suffix}`);
}

export async function approveFederationPairingRequest(requestId: string, approverUserId = "") {
  return apiJson<{ request: FederationPairingRequest; registration: FederationRegistration; trust: FederationTrust | null }>(
    `/api/federation/pairing/requests/${encodeURIComponent(requestId)}/approve`,
    {
      method: "POST",
      body: JSON.stringify({ approver_user_id: approverUserId }),
    },
  );
}

export async function rejectFederationPairingRequest(requestId: string, rejectedBy = "", reason = "") {
  return apiJson<{ request: FederationPairingRequest }>(
    `/api/federation/pairing/requests/${encodeURIComponent(requestId)}/reject`,
    {
      method: "POST",
      body: JSON.stringify({ rejected_by: rejectedBy, reason }),
    },
  );
}

export async function fetchFederationTrusts(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ trusts: FederationTrust[] }>(`/api/federation/pairing/trusts${suffix}`);
}

export async function revokeFederationTrust(trustId: string, revokedBy = "") {
  return apiJson<{ trust: FederationTrust }>(
    `/api/federation/pairing/trusts/${encodeURIComponent(trustId)}/revoke`,
    {
      method: "POST",
      body: JSON.stringify({ revoked_by: revokedBy }),
    },
  );
}

export async function updateFederationTrustAccess(trustId: string, accessLevel: GroupBridgeAccessLevel, updatedBy = "") {
  return apiJson<{ trust: FederationTrust }>(
    `/api/federation/pairing/trusts/${encodeURIComponent(trustId)}/access`,
    {
      method: "POST",
      body: JSON.stringify({ access_level: accessLevel, updated_by: updatedBy }),
    },
  );
}

export async function fetchFederationPairingOutbounds(groupId?: string) {
  const suffix = groupId ? `?group_id=${encodeURIComponent(groupId)}` : "";
  return apiJson<{ outbounds: FederationPairingOutbound[] }>(`/api/federation/pairing/outbounds${suffix}`);
}

export async function syncFederationPairingOutbound(outboundId: string) {
  return apiJson<{ outbound: FederationPairingOutbound }>(
    `/api/federation/pairing/outbounds/${encodeURIComponent(outboundId)}/sync`,
    { method: "POST" },
  );
}

export async function deleteFederationPairingOutbound(outboundId: string) {
  return apiJson<{ deleted: boolean }>(
    `/api/federation/pairing/outbounds/${encodeURIComponent(outboundId)}/delete`,
    { method: "POST" },
  );
}
