import { afterEach, describe, expect, it, vi } from "vitest";

import {
  fetchFederationDeliveryStatus,
  fetchFederationIdentity,
  fetchFederationPairingRequests,
  fetchFederationPairingOutbounds,
  syncFederationPairingOutbound,
  deleteFederationPairingOutbound,
  fetchFederationTrusts,
  approveFederationPairingRequest,
  createFederationPairingConnectionInfo,
  createFederationPairingInvite,
  createFederationPairingRequest,
  createFederationRemotePairingRequest,
  rejectFederationPairingRequest,
  refreshFederationTrustRemoteInfo,
  revokeFederationTrust,
  fetchFederationStatus,
  updateFederationTrustAccess,
  unregisterFederation,
} from "./federation";

function okResponse(result: unknown): Response {
  return new Response(JSON.stringify({ ok: true, result }));
}

describe("federation API client", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("status / delivery-status use the expected routes", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async () => okResponse({ registrations: [] }));
    await fetchFederationStatus("g1");
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe("/api/federation/status?group_id=g1");

    await fetchFederationDeliveryStatus("r1", "k1");
    expect(String(fetchMock.mock.calls[1]?.[0])).toBe("/api/federation/registrations/r1/deliveries/k1");

    await unregisterFederation("r1");
    const [url, init] = fetchMock.mock.calls[2] || [];
    expect(String(url)).toBe("/api/federation/unregister");
    expect(JSON.parse(String((init as RequestInit)?.body)).registration_id).toBe("r1");
  });

  it("pairing APIs use the federation session pairing routes", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async () => okResponse({}));

    await fetchFederationIdentity();
    await createFederationPairingInvite({
      groupId: "g1",
      ttlSeconds: 600,
    });
    await createFederationPairingConnectionInfo({
      groupId: "g1",
      inviteId: "pinv_1",
      issuerEndpoint: "https://issuer.example",
      issuerGroupTitle: "Issuer",
    });
    await createFederationPairingRequest({
      pairingCode: "ABCD-1234",
      requesterGroupId: "g_remote",
      requesterPeerId: "peer_remote",
    });
    await createFederationRemotePairingRequest({
      localGroupId: "g_remote",
      payload: { issuer_endpoint: "https://issuer.example", issuer_group_id: "g1", code: "WXYZ-9876" },
    });
    await fetchFederationPairingRequests("g1");
    await approveFederationPairingRequest("preq_1", "user-a");
    await rejectFederationPairingRequest("preq_2", "user-a", "no");
    await fetchFederationTrusts("g1");
    await fetchFederationPairingOutbounds("g_remote");
    await syncFederationPairingOutbound("pout_1");
    await deleteFederationPairingOutbound("pout_1");
    await updateFederationTrustAccess("ptrust_1", "read", "user-a");
    await refreshFederationTrustRemoteInfo("ptrust_1");
    await revokeFederationTrust("ptrust_1", "user-a");

    expect(String(fetchMock.mock.calls[0]?.[0])).toBe("/api/federation/pairing/identity");
    expect(String(fetchMock.mock.calls[1]?.[0])).toBe("/api/federation/pairing/invites");
    expect(JSON.parse(String((fetchMock.mock.calls[1]?.[1] as RequestInit)?.body))).toEqual({
      group_id: "g1",
      ttl_seconds: 600,
    });
    expect(String(fetchMock.mock.calls[2]?.[0])).toBe("/api/federation/pairing/connection-info");
    expect(JSON.parse(String((fetchMock.mock.calls[2]?.[1] as RequestInit)?.body))).toEqual({
      group_id: "g1",
      invite_id: "pinv_1",
      issuer_endpoint: "https://issuer.example",
      issuer_group_title: "Issuer",
    });
    expect(String(fetchMock.mock.calls[3]?.[0])).toBe("/api/federation/pairing/requests");
    expect(JSON.parse(String((fetchMock.mock.calls[3]?.[1] as RequestInit)?.body))).toEqual({
      pairing_code: "ABCD-1234",
      requester_group_id: "g_remote",
      requester_peer_id: "peer_remote",
    });
    expect(String(fetchMock.mock.calls[4]?.[0])).toBe("/api/federation/pairing/remote-requests");
    expect(JSON.parse(String((fetchMock.mock.calls[4]?.[1] as RequestInit)?.body))).toEqual({
      local_group_id: "g_remote",
      local_group_title: "",
      payload: { issuer_endpoint: "https://issuer.example", issuer_group_id: "g1", code: "WXYZ-9876" },
    });
    expect(String(fetchMock.mock.calls[5]?.[0])).toBe("/api/federation/pairing/requests?group_id=g1");
    expect(String(fetchMock.mock.calls[6]?.[0])).toBe("/api/federation/pairing/requests/preq_1/approve");
    expect(String(fetchMock.mock.calls[7]?.[0])).toBe("/api/federation/pairing/requests/preq_2/reject");
    expect(String(fetchMock.mock.calls[8]?.[0])).toBe("/api/federation/pairing/trusts?group_id=g1");
    expect(String(fetchMock.mock.calls[9]?.[0])).toBe("/api/federation/pairing/outbounds?group_id=g_remote");
    expect(String(fetchMock.mock.calls[10]?.[0])).toBe("/api/federation/pairing/outbounds/pout_1/sync");
    expect(String((fetchMock.mock.calls[10]?.[1] as RequestInit)?.method)).toBe("POST");
    expect(String(fetchMock.mock.calls[11]?.[0])).toBe("/api/federation/pairing/outbounds/pout_1/delete");
    expect(String((fetchMock.mock.calls[11]?.[1] as RequestInit)?.method)).toBe("POST");
    expect(String(fetchMock.mock.calls[12]?.[0])).toBe("/api/federation/pairing/trusts/ptrust_1/access");
    expect(JSON.parse(String((fetchMock.mock.calls[12]?.[1] as RequestInit)?.body))).toEqual({
      access_level: "read",
      updated_by: "user-a",
    });
    expect(String(fetchMock.mock.calls[13]?.[0])).toBe("/api/federation/pairing/trusts/ptrust_1/refresh");
    expect(String((fetchMock.mock.calls[13]?.[1] as RequestInit)?.method)).toBe("POST");
    expect(String(fetchMock.mock.calls[14]?.[0])).toBe("/api/federation/pairing/trusts/ptrust_1/revoke");
    expect(JSON.parse(String((fetchMock.mock.calls[14]?.[1] as RequestInit)?.body))).toEqual({ revoked_by: "user-a" });
  });
});
