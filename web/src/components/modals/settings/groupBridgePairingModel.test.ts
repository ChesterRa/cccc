import { describe, expect, it } from "vitest";

import type {
  GroupBridgePairingOutbound,
  GroupBridgePairingRequest,
  GroupBridgeRegistration,
  GroupBridgeTrust,
} from "../../../services/api/groupBridge";
import {
  buildConnectionInfoPayload,
  canCreateInvite,
  canSubmitPairingRequest,
  parseConnectionInfoInput,
  filterGroupBridgeSessionRegistrations,
  formatPeerLabel,
  formatRemoteGroupLabel,
  isLocalIssuerEndpoint,
  isSameInstancePairingInput,
  isSessionConnectionInfoInput,
  normalizeGroupBridgeAccessLevel,
  normalizeIssuerEndpoint,
  projectIncomingRequests,
  projectRecentOutbounds,
  projectPairingOverview,
  projectSyncableOutbounds,
  projectTrustedPeers,
  safePairingCodeText,
  userFacingPairingErrorKey,
  shouldUsePairingCodeHelp,
} from "./groupBridgePairingModel";

describe("groupBridgePairingModel", () => {
  it("gates invite creation on the current local group only", () => {
    expect(canCreateInvite({ groupId: "", busy: false })).toBe(false);
    expect(canCreateInvite({ groupId: "g1", busy: true })).toBe(false);
    expect(canCreateInvite({ groupId: "g1", busy: false, issuerEndpoint: "" })).toBe(false);
    expect(canCreateInvite({ groupId: "g1", busy: false, issuerEndpoint: "https://issuer.example" })).toBe(true);
  });

  it("gates pairing request submission on session connection info and local identity peer id", () => {
    expect(canSubmitPairingRequest({ pairingCode: "", requesterGroupId: "g1", requesterPeerId: "peer1", isRemote: true, busy: false })).toBe(false);
    expect(canSubmitPairingRequest({ pairingCode: "ABCD-1234", requesterGroupId: "", requesterPeerId: "peer1", isRemote: true, busy: false })).toBe(false);
    expect(canSubmitPairingRequest({ pairingCode: "ABCD-1234", requesterGroupId: "g1", requesterPeerId: "", isRemote: true, busy: false })).toBe(false);
    expect(canSubmitPairingRequest({ pairingCode: "ABCD-1234", requesterGroupId: "g1", requesterPeerId: "peer1", isRemote: true, busy: true })).toBe(false);
    expect(canSubmitPairingRequest({ pairingCode: "ABCD-1234", requesterGroupId: "g1", requesterPeerId: "peer1", isRemote: false, busy: false })).toBe(false);
    expect(canSubmitPairingRequest({ pairingCode: "ABCD-1234", requesterGroupId: "g1", requesterPeerId: "peer1", isRemote: true, busy: false })).toBe(true);
  });

  it("accepts either a raw pairing code or a JSON connection info payload", () => {
    expect(parseConnectionInfoInput("ABCD-1234")).toEqual({ pairingCode: "ABCD-1234" });
    expect(parseConnectionInfoInput(JSON.stringify({ pairing_code: "WXYZ-9876", group_id: "g_local", peer_id: "peer_host" }))).toMatchObject({
      pairingCode: "WXYZ-9876",
      remoteGroupId: "g_local",
      remotePeerId: "peer_host",
      isRemote: false,
    });
  });

  it("detects remote issuer endpoint payloads for Stage B submission", () => {
    const payload = {
      type: "cccc.group_bridge_session.connection_info",
      version: 2,
      issuer_endpoint: "https://issuer.example",
      issuer_group_id: "g_issuer",
      issuer_peer_id: "peer_issuer",
      code: "ABCD-1234",
      nonce: "pinv_1",
      integrity: "sha256:test",
    };
    expect(parseConnectionInfoInput(JSON.stringify(payload))).toEqual({
      pairingCode: "ABCD-1234",
      remoteGroupId: "g_issuer",
      remotePeerId: "peer_issuer",
      issuerEndpoint: "https://issuer.example",
      nonce: "pinv_1",
      integrity: "sha256:test",
      isRemote: true,
      payload,
    });
  });

  it("classifies same-instance fallback and local issuer endpoint hints", () => {
    expect(isSameInstancePairingInput(parseConnectionInfoInput("ABCD-1234"))).toBe(true);
    expect(isSameInstancePairingInput(parseConnectionInfoInput(JSON.stringify({
      issuer_endpoint: "https://issuer.example",
      code: "ABCD-1234",
      nonce: "pinv_1",
      integrity: "sha256:test",
    })))).toBe(false);
    expect(isSessionConnectionInfoInput(parseConnectionInfoInput(JSON.stringify({
      issuer_endpoint: "https://issuer.example",
      code: "ABCD-1234",
      nonce: "pinv_1",
      integrity: "sha256:test",
    })))).toBe(true);
    expect(isSessionConnectionInfoInput(parseConnectionInfoInput("ABCD-1234"))).toBe(false);
    expect(isLocalIssuerEndpoint("http://127.0.0.1:5555")).toBe(true);
    expect(isLocalIssuerEndpoint("https://cccc.example.com")).toBe(false);
  });

  it("normalizes issuer endpoints the same way as the backend integrity material", async () => {
    expect(normalizeIssuerEndpoint("cccc.example.com")).toBe("https://cccc.example.com");
    expect(normalizeIssuerEndpoint("https://cccc.example.com/some/path?token=secret#frag")).toBe("https://cccc.example.com");

    const payload = await buildConnectionInfoPayload({
      code: "ABCD-1234",
      groupId: "g_issuer",
      groupTitle: "Issuer Group",
      identity: { node_id: "node_issuer", peer_id: "peer_issuer" },
      issuerEndpoint: "cccc.example.com/some/path?token=secret#frag",
      inviteId: "pinv_1",
      expiresAt: "2026-06-15T04:00:00Z",
      digestHex: async (material) => {
        expect(material).toBe("https://cccc.example.com|g_issuer|Issuer Group|peer_issuer|ABCD-1234|2026-06-15T04:00:00Z|pinv_1");
        return "abc123";
      },
    });

    expect(payload.issuer_endpoint).toBe("https://cccc.example.com");
    expect(payload.issuer_group_title).toBe("Issuer Group");
    expect(payload.integrity).toBe("sha256:abc123");
  });

  it("rejects invalid issuer endpoints before creating connection info", () => {
    expect(() => normalizeIssuerEndpoint("ftp://cccc.example.com")).toThrow("issuer_endpoint must be an http(s) URL");
    expect(() => normalizeIssuerEndpoint("https://")).toThrow("issuer_endpoint must be an http(s) URL");
  });

  it("normalizes copied connection info before sending the pairing code", () => {
    expect(parseConnectionInfoInput('"ABCD-1234"')).toEqual({ pairingCode: "ABCD-1234" });
    expect(parseConnectionInfoInput("```json\n{\"pairing_code\":\"WXYZ-9876\",\"group_id\":\"g_local\"}\n```")).toMatchObject({
      pairingCode: "WXYZ-9876",
      remoteGroupId: "g_local",
      isRemote: false,
    });
    expect(parseConnectionInfoInput("{\n  \"type\": \"cccc.group_bridge_session.connection_info\",\n  \"pairing_code\": \" lmno-4567 \"\n}")).toMatchObject({
      pairingCode: "LMNO-4567",
      isRemote: false,
    });
  });

  it("formats peer labels without exposing secrets", () => {
    expect(formatPeerLabel({ remote_peer_id: "peer_remote", remote_group_id: "g_remote" })).toBe("g_remote / peer_remote");
    expect(formatPeerLabel({ remote_peer_id: "peer_remote", remote_group_id: "g_remote", remote_group_title: "Remote Group" })).toBe("Remote Group / peer_remote");
    expect(formatPeerLabel({ remote_peer_id: "", remote_group_id: "g_remote" })).toBe("g_remote");
    expect(formatPeerLabel({ remote_peer_id: "peer_remote", credential_ref: "sec_remote" })).not.toContain("sec_remote");
    expect(formatPeerLabel({}, "未知 peer")).toBe("未知 peer");
    expect(formatRemoteGroupLabel({ remote_peer_id: "peer_remote", remote_group_id: "g_remote", remote_group_title: "Remote Group" })).toBe("Remote Group");
    expect(formatRemoteGroupLabel({ remote_peer_id: "peer_remote", remote_group_id: "g_remote" })).toBe("g_remote");
    expect(formatRemoteGroupLabel({}, "未知远端工作组")).toBe("未知远端工作组");
  });

  it("filters Group Bridge session registrations from mixed status data", () => {
    const regs = [
      { registration_id: "r1", transport: "registry_hub" },
      { registration_id: "r2", transport: "group_bridge_session" },
    ] as GroupBridgeRegistration[];
    expect(filterGroupBridgeSessionRegistrations(regs).map((r) => r.registration_id)).toEqual(["r2"]);
  });

  it("projects pending incoming requests and trusted peers", () => {
    const requests = [
      { request_id: "preq_1", status: "pending", remote_peer_id: "peer_a", remote_group_id: "g_a" },
      { request_id: "preq_2", status: "approved", remote_peer_id: "peer_b", remote_group_id: "g_b" },
    ] as GroupBridgePairingRequest[];
    expect(projectIncomingRequests(requests).map((r) => r.request_id)).toEqual(["preq_1"]);

    const trusts = [
      { trust_id: "t1", status: "active", remote_peer_id: "peer_a", remote_group_id: "g_a" },
      { trust_id: "t2", status: "revoked", remote_peer_id: "peer_b", remote_group_id: "g_b" },
    ] as GroupBridgeTrust[];
    expect(projectTrustedPeers(trusts).map((t) => t.trust_id)).toEqual(["t1"]);
  });

  it("normalizes group bridge access levels conservatively", () => {
    expect(normalizeGroupBridgeAccessLevel("messages")).toBe("messages");
    expect(normalizeGroupBridgeAccessLevel("read")).toBe("read");
    expect(normalizeGroupBridgeAccessLevel("full")).toBe("full");
    expect(normalizeGroupBridgeAccessLevel("READ")).toBe("read");
    expect(normalizeGroupBridgeAccessLevel("unexpected")).toBe("messages");
    expect(normalizeGroupBridgeAccessLevel(undefined)).toBe("messages");
  });

  it("shows only the latest sent request per remote issuer", () => {
    const outbounds = [
      { outbound_id: "pout_old", issuer_endpoint: "https://issuer.example", issuer_group_id: "g_issuer", issuer_peer_id: "peer_issuer", status: "submitted", updated_at: "2026-06-15T01:00:00Z" },
      { outbound_id: "pout_other", issuer_endpoint: "https://other.example", issuer_group_id: "g_other", issuer_peer_id: "peer_other", status: "submitted", updated_at: "2026-06-15T02:00:00Z" },
      { outbound_id: "pout_new", issuer_endpoint: "https://issuer.example", issuer_group_id: "g_issuer", issuer_peer_id: "peer_issuer", status: "submitted", updated_at: "2026-06-15T03:00:00Z" },
    ] as GroupBridgePairingOutbound[];

    expect(projectRecentOutbounds(outbounds).map((outbound) => outbound.outbound_id)).toEqual(["pout_new", "pout_other"]);
  });

  it("hides approved sent requests from the recent outbound projection", () => {
    const outbounds = [
      { outbound_id: "pout_approved", issuer_endpoint: "https://issuer.example", issuer_group_id: "g_issuer", issuer_peer_id: "peer_issuer", status: "approved", updated_at: "2026-06-15T03:00:00Z" },
      { outbound_id: "pout_failed", issuer_endpoint: "https://failed.example", issuer_group_id: "g_failed", issuer_peer_id: "peer_failed", status: "failed", updated_at: "2026-06-15T02:00:00Z" },
      { outbound_id: "pout_rejected", issuer_endpoint: "https://rejected.example", issuer_group_id: "g_rejected", issuer_peer_id: "peer_rejected", status: "rejected", updated_at: "2026-06-15T01:00:00Z" },
    ] as GroupBridgePairingOutbound[];

    expect(projectRecentOutbounds(outbounds).map((outbound) => outbound.outbound_id)).toEqual(["pout_failed", "pout_rejected"]);
  });

  it("continues polling remote status for submitted and pending outbound requests", () => {
    const outbounds = [
      { outbound_id: "pout_submitted", status: "submitted" },
      { outbound_id: "pout_pending", status: "pending" },
      { outbound_id: "pout_approved", status: "approved" },
      { outbound_id: "pout_failed", status: "failed" },
      { outbound_id: "pout_rejected", status: "rejected" },
    ] as GroupBridgePairingOutbound[];

    expect(projectSyncableOutbounds(outbounds).map((outbound) => outbound.outbound_id)).toEqual(["pout_submitted", "pout_pending"]);
  });

  it("projects overview chips from identity and pairing lists", () => {
    const requests = [
      { request_id: "preq_1", status: "pending", remote_peer_id: "peer_a", remote_group_id: "g_a" },
      { request_id: "preq_2", status: "approved", remote_peer_id: "peer_b", remote_group_id: "g_b" },
    ] as GroupBridgePairingRequest[];
    const trusts = [
      { trust_id: "t1", status: "active", remote_peer_id: "peer_a", remote_group_id: "g_a" },
      { trust_id: "t2", status: "revoked", remote_peer_id: "peer_b", remote_group_id: "g_b" },
    ] as GroupBridgeTrust[];

    expect(projectPairingOverview({ identity: { node_id: "node1", peer_id: "peer1" }, requests, trusts })).toEqual({
      identityReady: true,
      pendingCount: 1,
      trustedCount: 1,
    });
    expect(projectPairingOverview({ identity: null, requests: [], trusts: [] }).identityReady).toBe(false);
  });

  it("redacts pairing code when asked to render safe text", () => {
    expect(safePairingCodeText("ABCD-1234")).toBe("ABCD-1234");
    expect(safePairingCodeText("")).toBe("Code unavailable");
  });

  it("detects pairing code errors that need user-facing guidance", () => {
    expect(shouldUsePairingCodeHelp("pairing code not found")).toBe(true);
    expect(shouldUsePairingCodeHelp("pairing code expired")).toBe(true);
    expect(shouldUsePairingCodeHelp("network failed")).toBe(false);
  });

  it("maps backend issuer endpoint policy errors to user-facing locale keys", () => {
    expect(userFacingPairingErrorKey("unsafe issuer_endpoint is not allowed")).toBe("group_bridge.unsafeIssuerEndpointBlocked");
    expect(userFacingPairingErrorKey("remote pairing request failed")).toBeNull();
  });
});
