import type {
  FederationIdentity,
  FederationPairingOutbound,
  FederationPairingRequest,
  FederationRegistration,
  FederationTrust,
} from "../../../services/api/federation";

export type DigestHex = (value: string) => Promise<string>;

type PeerLike = {
  remote_peer_id?: string;
  remote_group_id?: string;
  remote_group_title?: string;
  remote_endpoint?: string;
  credential_ref?: string;
};

export function canCreateInvite(opts: {
  groupId: string;
  busy: boolean;
  issuerEndpoint?: string;
}): boolean {
  return (
    String(opts.groupId || "").trim().length > 0 &&
    String(opts.issuerEndpoint || "").trim().length > 0 &&
    !opts.busy
  );
}

export function canSubmitPairingRequest(opts: {
  pairingCode: string;
  requesterGroupId: string;
  requesterPeerId: string;
  isRemote?: boolean;
  busy: boolean;
}): boolean {
  return (
    String(opts.pairingCode || "").trim().length > 0 &&
    String(opts.requesterGroupId || "").trim().length > 0 &&
    String(opts.requesterPeerId || "").trim().length > 0 &&
    opts.isRemote === true &&
    !opts.busy
  );
}

export function formatPeerLabel(peer: PeerLike, fallback = "Unknown peer"): string {
  const remotePeerId = String(peer.remote_peer_id || "").trim();
  const remoteGroupId = String(peer.remote_group_id || "").trim();
  const remoteGroupTitle = String(peer.remote_group_title || "").trim();
  const groupLabel = remoteGroupTitle || remoteGroupId;
  if (remotePeerId && groupLabel) return `${groupLabel} / ${remotePeerId}`;
  if (groupLabel) return groupLabel;
  return remotePeerId || remoteGroupId || fallback;
}

export function formatRemoteInstanceLabel(peer: PeerLike, fallback = "Unknown CCCC"): string {
  return String(peer.remote_endpoint || "").trim() || fallback;
}

export function parseConnectionInfoInput(raw: string): {
  pairingCode: string;
  remoteGroupId?: string;
  remotePeerId?: string;
  issuerEndpoint?: string;
  nonce?: string;
  integrity?: string;
  isRemote?: boolean;
  payload?: Record<string, unknown>;
} {
  const value = stripCodeFence(String(raw || "").trim());
  if (!value) return { pairingCode: "" };
  try {
    const parsed = JSON.parse(value) as Record<string, unknown> | string;
    if (typeof parsed === "string") return { pairingCode: normalizePairingCode(parsed) };
    return projectConnectionInfoPayload(parsed);
  } catch {
    return { pairingCode: normalizePairingCode(value) };
  }
}

function stripCodeFence(value: string): string {
  const match = value.match(/^```(?:json)?\s*([\s\S]*?)\s*```$/i);
  return (match?.[1] || value).trim();
}

function normalizePairingCode(value: string): string {
  return String(value || "").trim().replace(/^['"]|['"]$/g, "").trim().toUpperCase();
}

function projectConnectionInfoPayload(parsed: Record<string, unknown>): {
  pairingCode: string;
  remoteGroupId?: string;
  remotePeerId?: string;
  issuerEndpoint?: string;
  nonce?: string;
  integrity?: string;
  isRemote?: boolean;
  payload?: Record<string, unknown>;
} {
  const issuerEndpoint = String(parsed.issuer_endpoint || parsed.issuerEndpoint || "").trim() || undefined;
  const pairingCode = normalizePairingCode(String(parsed.code || parsed.pairing_code || parsed.pairingCode || ""));
  return {
    pairingCode,
    remoteGroupId: String(parsed.issuer_group_id || parsed.issuerGroupId || parsed.group_id || parsed.groupId || "").trim() || undefined,
    remotePeerId: String(parsed.issuer_peer_id || parsed.issuerPeerId || parsed.peer_id || parsed.peerId || "").trim() || undefined,
    issuerEndpoint,
    nonce: String(parsed.nonce || parsed.invite_id || parsed.inviteId || "").trim() || undefined,
    integrity: String(parsed.integrity || "").trim() || undefined,
    isRemote: Boolean(issuerEndpoint),
    payload: issuerEndpoint ? parsed : undefined,
  };
}

export function filterFederationSessionRegistrations(registrations: FederationRegistration[] | undefined | null): FederationRegistration[] {
  return (registrations || []).filter((registration) => String(registration.transport || "") === "federation_session");
}

export function projectIncomingRequests(
  requests: FederationPairingRequest[] | undefined | null,
): FederationPairingRequest[] {
  return (requests || []).filter((request) => String(request.status || "") === "pending");
}

export function projectTrustedPeers(trusts: FederationTrust[] | undefined | null): FederationTrust[] {
  return (trusts || []).filter((trust) => String(trust.status || "") === "active");
}

export function projectRecentOutbounds(outbounds: FederationPairingOutbound[] | undefined | null, limit = 3): FederationPairingOutbound[] {
  const latestByIssuer = new Map<string, FederationPairingOutbound>();
  for (const outbound of outbounds || []) {
    if (String(outbound.status || "").trim() === "approved") continue;
    const key = [outbound.issuer_endpoint, outbound.issuer_group_id, outbound.issuer_peer_id]
      .map((part) => String(part || "").trim())
      .join("|") || outbound.outbound_id;
    const existing = latestByIssuer.get(key);
    if (!existing || outboundSortTime(outbound) >= outboundSortTime(existing)) {
      latestByIssuer.set(key, outbound);
    }
  }
  return [...latestByIssuer.values()]
    .sort((a, b) => outboundSortTime(b) - outboundSortTime(a) || String(b.outbound_id || "").localeCompare(String(a.outbound_id || "")))
    .slice(0, Math.max(0, limit));
}

export function projectSyncableOutbounds(outbounds: FederationPairingOutbound[] | undefined | null): FederationPairingOutbound[] {
  return (outbounds || []).filter((outbound) => {
    const status = String(outbound.status || "").trim();
    return status === "submitted" || status === "pending";
  });
}

function outboundSortTime(outbound: FederationPairingOutbound): number {
  const value = Date.parse(String(outbound.updated_at || outbound.created_at || ""));
  return Number.isFinite(value) ? value : 0;
}

export function projectPairingOverview(opts: {
  identity: FederationIdentity | null | undefined;
  requests: FederationPairingRequest[] | undefined | null;
  trusts: FederationTrust[] | undefined | null;
}) {
  return {
    identityReady: Boolean(String(opts.identity?.peer_id || "").trim()),
    pendingCount: projectIncomingRequests(opts.requests).length,
    trustedCount: projectTrustedPeers(opts.trusts).length,
  };
}

export function safePairingCodeText(code: string | undefined | null, fallback = "Code unavailable"): string {
  return String(code || "").trim() || fallback;
}

export function shouldUsePairingCodeHelp(errorMessage: string | undefined | null): boolean {
  const msg = String(errorMessage || "").toLowerCase();
  return msg.includes("pairing code not found") || msg.includes("pairing code expired") || msg.includes("pairing code already used");
}

export function userFacingPairingErrorKey(errorMessage: string | undefined | null): string | null {
  const msg = String(errorMessage || "").toLowerCase();
  if (msg.includes("unsafe issuer_endpoint") || msg.includes("link-local") || msg.includes("metadata")) {
    return "federation.unsafeIssuerEndpointBlocked";
  }
  return null;
}

export function isSameInstancePairingInput(parsed: { pairingCode: string; isRemote?: boolean }): boolean {
  return Boolean(String(parsed.pairingCode || "").trim() && !parsed.isRemote);
}

export function isSessionConnectionInfoInput(parsed: { pairingCode: string; isRemote?: boolean; issuerEndpoint?: string; nonce?: string; integrity?: string }): boolean {
  return Boolean(
    String(parsed.pairingCode || "").trim() &&
    parsed.isRemote === true &&
    String(parsed.issuerEndpoint || "").trim() &&
    String(parsed.nonce || "").trim() &&
    String(parsed.integrity || "").trim(),
  );
}

export function isLocalIssuerEndpoint(endpoint: string): boolean {
  try {
    const parsed = new URL(normalizeIssuerEndpoint(endpoint));
    const host = parsed.hostname.toLowerCase();
    return host === "localhost" || host === "127.0.0.1" || host === "::1";
  } catch {
    return false;
  }
}

export function normalizeIssuerEndpoint(endpoint: string): string {
  const value = String(endpoint || "").trim();
  if (!value) throw new Error("issuer_endpoint is required");
  let parsed: URL;
  try {
    parsed = new URL(value.includes("://") ? value : `https://${value}`);
  } catch {
    throw new Error("issuer_endpoint must be an http(s) URL");
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
    throw new Error("issuer_endpoint must be an http(s) URL");
  }
  if (!parsed.hostname.trim()) {
    throw new Error("issuer_endpoint host is required");
  }
  return `${parsed.protocol}//${parsed.host}`.replace(/\/+$/, "");
}

export async function buildConnectionInfoPayload(opts: {
  code: string;
  groupId: string;
  groupTitle?: string;
  identity: FederationIdentity | null;
  issuerEndpoint: string;
  inviteId: string;
  expiresAt: string;
  digestHex: DigestHex;
}): Promise<Record<string, unknown>> {
  const normalizedEndpoint = normalizeIssuerEndpoint(opts.issuerEndpoint);
  const groupTitle = String(opts.groupTitle || "").trim();
  const material = [
    normalizedEndpoint,
    opts.groupId,
    groupTitle,
    opts.identity?.peer_id || "",
    opts.code,
    opts.expiresAt,
    opts.inviteId,
  ].join("|");
  return {
    type: "cccc.federation_session.connection_info",
    version: 2,
    issuer_endpoint: normalizedEndpoint,
    issuer_group_id: opts.groupId,
    issuer_group_title: groupTitle,
    issuer_peer_id: opts.identity?.peer_id || "",
    issuer_node_id: opts.identity?.node_id || "",
    code: opts.code,
    pairing_code: opts.code,
    expires_at: opts.expiresAt,
    nonce: opts.inviteId,
    integrity: `sha256:${await opts.digestHex(material)}`,
  };
}
