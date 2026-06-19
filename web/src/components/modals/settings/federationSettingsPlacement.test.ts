import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

function readSource(relPath: string): string {
  const url = new URL(relPath, import.meta.url);
  return readFileSync(fileURLToPath(url), "utf-8");
}

function readJson(relPath: string): unknown {
  return JSON.parse(readSource(relPath));
}

describe("federation settings placement", () => {
  it("connections is a recognized group settings tab and federation remains global legacy", () => {
    expect(readSource("./types.ts")).toContain('| "federation"');
    expect(readSource("./types.ts")).toContain('| "connections"');
    expect(readSource("./settingsLastLocation.ts")).toContain('"federation"');
    expect(readSource("./settingsLastLocation.ts")).toContain('"connections"');
  });

  it("SettingsModal registers group Connections and keeps Global Federation separate", () => {
    const src = readSource("../../SettingsModal.tsx");
    expect(src).toContain('id: "federation"');
    expect(src).toContain('id: "connections"');
    expect(src).toContain('activeTab === "federation"');
    expect(src).toContain('activeTab === "connections"');
    expect(src).toContain("FederationRegistrationSection");
    expect(src).toContain("FederationConnectionsSection");
  });

  it("WebAccessTab no longer embeds the federation form", () => {
    const src = readSource("./WebAccessTab.tsx");
    expect(src).not.toContain("FederationRegistrationSection");
  });

  it("FederationRegistrationSection asks for a credential reference, not a token", () => {
    const src = readSource("./FederationRegistrationSection.tsx");
    expect(readSource("../../../i18n/locales/en/settings.json")).toContain("Credential reference");
    expect(src).not.toContain("Credential / token");
  });

  it("Global Federation keeps HTTP URL target and does not mount the libp2p pairing workbench", () => {
    const src = readSource("./FederationRegistrationSection.tsx");
    expect(src).toContain("FederationHttpRegistrationSection");
    expect(src).toContain('useTranslation("settings")');
    expect(src).toContain('t("federation.libp2pManagedPerGroup")');
    expect(src).not.toContain("FederationLibp2pPairingSection");
    expect(src).not.toContain('useState<FederationMode>');
    expect(src).not.toContain("HTTP URL target");
    expect(src).not.toContain("libp2p pairing");
    expect(src).not.toContain("Remote URL");
    expect(src).not.toContain("Credential reference");
    expect(src).not.toContain("registerFederation");
  });

  it("group Connections owns libp2p pairing and always filters by current group", () => {
    const src = readSource("./FederationConnectionsSection.tsx");
    expect(src).toContain("FederationLibp2pPairingSection");
    expect(src).toContain("groupId");
    expect(src).toContain("fetchFederationPairingRequests(groupId)");
    expect(src).toContain("fetchFederationTrusts(groupId)");
    expect(src).toContain("fetchFederationStatus(groupId)");
  });

  it("group Connections syncs submitted and pending outbound requests on refresh", () => {
    const src = readSource("./FederationConnectionsSection.tsx");
    expect(src).toContain("projectSyncableOutbounds");
    expect(src).toContain("syncFederationPairingOutbound(outbound.outbound_id)");
    expect(src).not.toContain('String(outbound.status || "") === "submitted"');
  });

  it("HTTP registration section is explicitly legacy URL-target mode", () => {
    const src = readSource("./FederationHttpRegistrationSection.tsx");
    expect(src).toContain('useTranslation("settings")');
    expect(src).not.toContain("Legacy HTTP URL target");
    expect(src).not.toContain("Credential reference");
    expect(src).not.toContain("Remote URL");
    expect(src).toContain("peer_cccc_http");
  });

  it("libp2p pairing section does not expose direct registration fields or call direct register", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).not.toContain("Remote URL");
    expect(src).not.toContain("Credential reference");
    expect(src).not.toContain("Remote group URL");
    expect(src).not.toContain("DHT");
    expect(src).not.toContain("relay");
    expect(src).not.toContain("hole punching");
    expect(src).not.toContain("registerFederation");
    expect(src).toContain("createFederationPairingInvite");
    expect(src).toContain("createFederationRemotePairingRequest");
    expect(src).toContain("approveFederationPairingRequest");
  });

  it("libp2p connection info copy uses the shared clipboard fallback", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain("copyTextToClipboard");
    expect(src).not.toContain("navigator.clipboard?.writeText");
  });

  it("libp2p pairing is cross-instance first and demotes same-instance codes", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain('t("federation.connectRemoteCcccGroup")');
    expect(src).toContain('t("federation.issuerEndpoint")');
    expect(src).toContain('t("federation.issuerEndpointLocalOnlyHelp")');
    expect(src).toContain('t("federation.sameInstanceFallback")');
    expect(src).toContain("isSameInstancePairingInput");
    const modelSrc = readSource("./federationPairingModel.ts");
    expect(modelSrc).toContain("issuer_endpoint");
    expect(modelSrc).toContain("expires_at");
    expect(modelSrc).toContain("nonce");
    expect(modelSrc).toContain("integrity");
    expect(src).not.toContain('placeholder="ABCD-1234"');
  });

  it("libp2p connection info is generated by the backend after invite creation", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain("normalizeIssuerEndpoint(issuerEndpoint)");
    expect(src).toContain("createFederationPairingConnectionInfo");
    expect(src.indexOf("createFederationPairingInvite")).toBeLessThan(src.indexOf("createFederationPairingConnectionInfo"));
    expect(src).not.toContain("assertSha256Available();");
    expect(src).not.toContain("crypto.subtle.digest");
    expect(src).toContain('t("federation.issuerEndpointInvalid")');
    expect(src).toContain("userFacingPairingErrorKey");
    expect(src).not.toContain('setInviteError(infoResp.error.message || t("federation.createInviteFailed"))');
  });

  it("libp2p connection info includes the current group title for remote display", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain("issuerGroupTitle");
    expect(src).toContain("issuerGroupTitle: currentGroupTitle || currentGroupId");
  });

  it("libp2p pairing submits remote issuer payloads through the remote pairing endpoint", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain("parsed.isRemote && parsed.payload");
    expect(src).toContain("createFederationRemotePairingRequest");
    expect(src).toContain('t("federation.remotePayloadDetected")');
    expect(src).toContain('t("federation.remotePayloadTarget"');
  });

  it("libp2p pairing revocation invalidates composer federation route suggestions", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    const revokeStart = src.indexOf("const revokeTrustedPeer");
    expect(revokeStart).toBeGreaterThan(0);
    const revokeSrc = src.slice(revokeStart, src.indexOf("const deleteOutbound", revokeStart));
    expect(revokeSrc).toContain("publishFederationPairingChanged(currentGroupId)");
    expect(revokeSrc.indexOf("revokeFederationTrust(tid)")).toBeLessThan(revokeSrc.indexOf("publishFederationPairingChanged(currentGroupId)"));
  });

  it("libp2p pairing section uses the current group without local group selection", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain("currentGroupId");
    expect(src).not.toContain("setSelectedGroupId");
    expect(src).not.toContain('t("federation.localGroup")');
    expect(src).not.toContain('t("federation.selectGroup")');
    expect(src).not.toContain('t("federation.peerGroupId")');
  });

  it("libp2p pairing is an approval workbench, not a direct registration form", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain('useTranslation("settings")');
    expect(src).toContain('t("federation.connectRemoteCcccGroup")');
    expect(src).toContain('t("federation.createConnectionInfo")');
    expect(src).toContain('t("federation.pasteConnectionInfo")');
    expect(src).toContain('t("federation.pendingBadge")');
    expect(src).toContain('t("federation.trustedBadge")');
    expect(src).toContain('t("federation.revokeTrust")');
    expect(src).not.toContain("This node");
    expect(src).not.toContain("Create invite");
    expect(src).not.toContain("Submit pairing request");
    expect(src).not.toContain("Incoming requests");
    expect(src).not.toContain("Trusted peers");
    expect(src).not.toContain("Invite another CCCC");
    expect(src).not.toContain("Join with a pairing code");
    expect(src).not.toContain("invite/request metadata, not direct registration");
    expect(src).toContain("<details");
  });

  it("libp2p pairing puts workbench actions before local diagnostics", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain('t("federation.localDiagnostics")');
    expect(src.indexOf('t("federation.connectRemoteCcccGroup")')).toBeLessThan(src.indexOf('t("federation.localDiagnostics")'));
    expect(src.indexOf('t("federation.createConnectionInfo")')).toBeLessThan(src.indexOf('t("federation.localDiagnostics")'));
    expect(src.indexOf('t("federation.pasteConnectionInfo")')).toBeLessThan(src.indexOf('t("federation.localDiagnostics")'));
    expect(src.indexOf('t("federation.incomingRequests")')).toBeLessThan(src.indexOf('t("federation.localDiagnostics")'));
    expect(src.indexOf('t("federation.trustedRemoteGroups")')).toBeLessThan(src.indexOf('t("federation.localDiagnostics")'));
  });

  it("libp2p pairing uses local identity as request metadata instead of requester peer input", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    expect(src).toContain("requesterPeerId: localPeerId");
    expect(src).not.toContain("setRequestPeerId");
    expect(src).not.toContain('t("federation.yourPeerId")');
  });

  it("libp2p diagnostics do not present invite metadata addresses as local addresses", () => {
    const src = readSource("./FederationLibp2pPairingSection.tsx");
    const diagnosticsStart = src.indexOf('t("federation.localDiagnostics")');
    expect(diagnosticsStart).toBeGreaterThan(0);
    const diagnosticsSrc = src.slice(diagnosticsStart);
    expect(diagnosticsSrc).toContain('t("federation.localMultiaddrsUnavailable")');
    expect(diagnosticsSrc).not.toContain("multiaddrs.join");
    expect(diagnosticsSrc).not.toContain("multiaddrsText");
  });

  it("Federation Settings user-facing copy lives in settings locale files", () => {
    const locales = [
      readJson("../../../i18n/locales/en/settings.json"),
      readJson("../../../i18n/locales/zh/settings.json"),
      readJson("../../../i18n/locales/ja/settings.json"),
    ] as Array<{ federation?: Record<string, unknown> }>;
    for (const locale of locales) {
      expect(locale.federation).toBeTruthy();
      expect(locale.federation?.modeHttp).toBeTruthy();
      expect(locale.federation?.modeLibp2p).toBeTruthy();
      expect(locale.federation?.connectRemoteCcccGroup).toBeTruthy();
      expect(locale.federation?.createConnectionInfo).toBeTruthy();
      expect(locale.federation?.issuerEndpoint).toBeTruthy();
      expect(locale.federation?.issuerEndpointHelp).toBeTruthy();
      expect(locale.federation?.issuerEndpointLocalOnlyHelp).toBeTruthy();
      expect(locale.federation?.issuerEndpointInvalid).toBeTruthy();
      expect(locale.federation?.unsafeIssuerEndpointBlocked).toBeTruthy();
      expect(locale.federation?.sha256Unavailable).toBeTruthy();
      expect(locale.federation?.pasteConnectionInfo).toBeTruthy();
      expect(locale.federation?.remotePayloadPlaceholder).toBeTruthy();
      expect(locale.federation?.remotePayloadDetected).toBeTruthy();
      expect(locale.federation?.remotePayloadTarget).toBeTruthy();
      expect(locale.federation?.sameInstanceFallback).toBeTruthy();
      expect(locale.federation?.sameInstanceFallbackDetected).toBeTruthy();
      expect(locale.federation?.sameInstanceFallbackHelp).toBeTruthy();
      expect(locale.federation?.sameInstanceFallbackAdvancedHelp).toBeTruthy();
      expect(locale.federation?.incomingRequests).toBeTruthy();
      expect(locale.federation?.trustedPeers).toBeTruthy();
      expect(locale.federation?.pendingBadge).toBeTruthy();
      expect(locale.federation?.trustedBadge).toBeTruthy();
      expect(locale.federation?.advancedInviteMetadata).toBeTruthy();
      expect(locale.federation?.localDiagnostics).toBeTruthy();
      expect(locale.federation?.localDiagnosticsHelp).toBeTruthy();
      expect(locale.federation?.localMultiaddrsUnavailable).toBeTruthy();
      expect(locale.federation?.legacyHttpTitle).toBeTruthy();
      expect(locale.federation?.libp2pManagedPerGroup).toBeTruthy();
      expect(locale.tabs?.connections).toBeTruthy();
    }
  });
});
