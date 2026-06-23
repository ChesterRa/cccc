import { afterEach, describe, expect, it, vi } from "vitest";

import {
  fetchGroupBridgeDeliveryStatus,
  fetchGroupBridgeIdentity,
  fetchGroupBridgePairingRequests,
  fetchGroupBridgePairingOutbounds,
  syncGroupBridgePairingOutbound,
  deleteGroupBridgePairingOutbound,
  fetchGroupBridgeTrusts,
  approveGroupBridgePairingRequest,
  createGroupBridgePairingConnectionInfo,
  createGroupBridgePairingInvite,
  createGroupBridgePairingRequest,
  createGroupBridgeRemotePairingRequest,
  rejectGroupBridgePairingRequest,
  refreshGroupBridgeTrustRemoteInfo,
  revokeGroupBridgeTrust,
  fetchGroupBridgeStatus,
  updateGroupBridgeTrustAccess,
  unregisterGroupBridge,
} from "./groupBridge";

function okResponse(result: unknown): Response {
  return new Response(JSON.stringify({ ok: true, result }));
}

describe("group_bridge API client", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("status / delivery-status use the expected routes", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async () => okResponse({ registrations: [] }));
    await fetchGroupBridgeStatus("g1");
    expect(String(fetchMock.mock.calls[0]?.[0])).toBe("/api/group-bridge/status?group_id=g1");

    await fetchGroupBridgeDeliveryStatus("r1", "k1");
    expect(String(fetchMock.mock.calls[1]?.[0])).toBe("/api/group-bridge/registrations/r1/deliveries/k1");

    await unregisterGroupBridge("r1");
    const [url, init] = fetchMock.mock.calls[2] || [];
    expect(String(url)).toBe("/api/group-bridge/unregister");
    expect(JSON.parse(String((init as RequestInit)?.body)).registration_id).toBe("r1");
  });

  it("pairing APIs use the Group Bridge session pairing routes", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const fetchMock = vi.spyOn(globalThis, "fetch").mockImplementation(async () => okResponse({}));

    await fetchGroupBridgeIdentity();
    await createGroupBridgePairingInvite({
      groupId: "g1",
      ttlSeconds: 600,
    });
    await createGroupBridgePairingConnectionInfo({
      groupId: "g1",
      inviteId: "pinv_1",
      issuerEndpoint: "https://issuer.example",
      issuerGroupTitle: "Issuer",
    });
    await createGroupBridgePairingRequest({
      pairingCode: "ABCD-1234",
      requesterGroupId: "g_remote",
      requesterPeerId: "peer_remote",
    });
    await createGroupBridgeRemotePairingRequest({
      localGroupId: "g_remote",
      payload: { issuer_endpoint: "https://issuer.example", issuer_group_id: "g1", code: "WXYZ-9876" },
    });
    await fetchGroupBridgePairingRequests("g1");
    await approveGroupBridgePairingRequest("preq_1", "user-a");
    await rejectGroupBridgePairingRequest("preq_2", "user-a", "no");
    await fetchGroupBridgeTrusts("g1");
    await fetchGroupBridgePairingOutbounds("g_remote");
    await syncGroupBridgePairingOutbound("pout_1");
    await deleteGroupBridgePairingOutbound("pout_1");
    await updateGroupBridgeTrustAccess("ptrust_1", "read", "user-a");
    await refreshGroupBridgeTrustRemoteInfo("ptrust_1");
    await revokeGroupBridgeTrust("ptrust_1", "user-a");

    expect(String(fetchMock.mock.calls[0]?.[0])).toBe("/api/group-bridge/pairing/identity");
    expect(String(fetchMock.mock.calls[1]?.[0])).toBe("/api/group-bridge/pairing/invites");
    expect(JSON.parse(String((fetchMock.mock.calls[1]?.[1] as RequestInit)?.body))).toEqual({
      group_id: "g1",
      ttl_seconds: 600,
    });
    expect(String(fetchMock.mock.calls[2]?.[0])).toBe("/api/group-bridge/pairing/connection-info");
    expect(JSON.parse(String((fetchMock.mock.calls[2]?.[1] as RequestInit)?.body))).toEqual({
      group_id: "g1",
      invite_id: "pinv_1",
      issuer_endpoint: "https://issuer.example",
      issuer_group_title: "Issuer",
    });
    expect(String(fetchMock.mock.calls[3]?.[0])).toBe("/api/group-bridge/pairing/requests");
    expect(JSON.parse(String((fetchMock.mock.calls[3]?.[1] as RequestInit)?.body))).toEqual({
      pairing_code: "ABCD-1234",
      requester_group_id: "g_remote",
      requester_peer_id: "peer_remote",
    });
    expect(String(fetchMock.mock.calls[4]?.[0])).toBe("/api/group-bridge/pairing/remote-requests");
    expect(JSON.parse(String((fetchMock.mock.calls[4]?.[1] as RequestInit)?.body))).toEqual({
      local_group_id: "g_remote",
      local_group_title: "",
      payload: { issuer_endpoint: "https://issuer.example", issuer_group_id: "g1", code: "WXYZ-9876" },
    });
    expect(String(fetchMock.mock.calls[5]?.[0])).toBe("/api/group-bridge/pairing/requests?group_id=g1");
    expect(String(fetchMock.mock.calls[6]?.[0])).toBe("/api/group-bridge/pairing/requests/preq_1/approve");
    expect(String(fetchMock.mock.calls[7]?.[0])).toBe("/api/group-bridge/pairing/requests/preq_2/reject");
    expect(String(fetchMock.mock.calls[8]?.[0])).toBe("/api/group-bridge/pairing/trusts?group_id=g1");
    expect(String(fetchMock.mock.calls[9]?.[0])).toBe("/api/group-bridge/pairing/outbounds?group_id=g_remote");
    expect(String(fetchMock.mock.calls[10]?.[0])).toBe("/api/group-bridge/pairing/outbounds/pout_1/sync");
    expect(String((fetchMock.mock.calls[10]?.[1] as RequestInit)?.method)).toBe("POST");
    expect(String(fetchMock.mock.calls[11]?.[0])).toBe("/api/group-bridge/pairing/outbounds/pout_1/delete");
    expect(String((fetchMock.mock.calls[11]?.[1] as RequestInit)?.method)).toBe("POST");
    expect(String(fetchMock.mock.calls[12]?.[0])).toBe("/api/group-bridge/pairing/trusts/ptrust_1/access");
    expect(JSON.parse(String((fetchMock.mock.calls[12]?.[1] as RequestInit)?.body))).toEqual({
      access_level: "read",
      updated_by: "user-a",
    });
    expect(String(fetchMock.mock.calls[13]?.[0])).toBe("/api/group-bridge/pairing/trusts/ptrust_1/refresh");
    expect(String((fetchMock.mock.calls[13]?.[1] as RequestInit)?.method)).toBe("POST");
    expect(String(fetchMock.mock.calls[14]?.[0])).toBe("/api/group-bridge/pairing/trusts/ptrust_1/revoke");
    expect(JSON.parse(String((fetchMock.mock.calls[14]?.[1] as RequestInit)?.body))).toEqual({ revoked_by: "user-a" });
  });
});
