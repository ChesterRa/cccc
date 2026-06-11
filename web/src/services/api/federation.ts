import { apiJson } from "./base";

// Federation remote-send client.
//
// Security: the remote credential is only ever sent in a request body for
// verify/register. It is never written to localStorage/sessionStorage by this
// module (the session access token is handled separately by base.getAuthHeaders).

export interface FederationRegistration {
  registration_id: string;
  group_id: string;
  url: string;
  transport: string;
  remote_group_id: string;
  credential_ref: string;
  user_id: string;
  status: string;
  created_at: string;
  updated_at: string;
  last_sync_at?: string | null;
  last_error?: string | null;
}

export interface FederationVerifyResult {
  verified: boolean;
  group_id: string;
  normalized_url: string;
  transport: string;
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

export interface FederationTargetInput {
  groupId: string;
  url: string;
  /** Sent only in the request body; never persisted client-side. */
  credentialRef?: string;
  transport?: string;
  remoteGroupId?: string;
}

export function buildFederationTargetBody(input: FederationTargetInput): Record<string, unknown> {
  return {
    group_id: input.groupId,
    url: input.url,
    transport: input.transport ?? "peer_cccc_http",
    remote_group_id: input.remoteGroupId ?? "",
    credential_ref: input.credentialRef ?? "",
  };
}

export async function verifyFederation(input: FederationTargetInput) {
  return apiJson<FederationVerifyResult>("/api/federation/verify", {
    method: "POST",
    body: JSON.stringify(buildFederationTargetBody(input)),
  });
}

export async function registerFederation(input: FederationTargetInput) {
  return apiJson<{ registration: FederationRegistration }>("/api/federation/register", {
    method: "POST",
    body: JSON.stringify(buildFederationTargetBody(input)),
  });
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
