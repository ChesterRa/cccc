import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshIcon } from "../../Icons";
import * as api from "../../../services/api";
import type { FederationIdentity, FederationPairingOutbound, FederationPairingRequest, FederationRegistration, FederationTrust } from "../../../services/api/federation";
import {
  canCreateInvite,
  canSubmitPairingRequest,
  filterLibp2pRegistrations,
  formatPeerLabel,
  formatRemoteInstanceLabel,
  isLocalIssuerEndpoint,
  isSameInstancePairingInput,
  normalizeIssuerEndpoint,
  parseConnectionInfoInput,
  projectIncomingRequests,
  projectPairingOverview,
  projectRecentOutbounds,
  projectTrustedPeers,
  shouldUsePairingCodeHelp,
  userFacingPairingErrorKey,
} from "./federationPairingModel";
import {
  dangerButtonClass,
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import { publishFederationPairingChanged } from "../../../utils/federationPairingEvents";
import { copyTextToClipboard } from "../../../utils/copy";

interface Props {
  isDark: boolean;
  currentGroupId: string;
  currentGroupTitle?: string;
  registrations: FederationRegistration[];
  identity: FederationIdentity | null;
  requests: FederationPairingRequest[];
  trusts: FederationTrust[];
  outbounds: FederationPairingOutbound[];
  refreshPairing: () => Promise<void>;
}

function defaultIssuerEndpoint(): string {
  return typeof window !== "undefined" ? window.location.origin : "";
}

export function FederationLibp2pPairingSection({
  isDark,
  currentGroupId,
  currentGroupTitle,
  registrations,
  identity,
  requests,
  trusts,
  outbounds,
  refreshPairing,
}: Props) {
  const { t } = useTranslation("settings");
  const [connectionInput, setConnectionInput] = useState("");
  const [createdInfo, setCreatedInfo] = useState("");
  const [inviteError, setInviteError] = useState("");
  const [requestError, setRequestError] = useState("");
  const [reviewError, setReviewError] = useState("");
  const [copyNotice, setCopyNotice] = useState("");
  const [requestNotice, setRequestNotice] = useState("");
  const [issuerEndpoint, setIssuerEndpoint] = useState(defaultIssuerEndpoint);
  const [busy, setBusy] = useState(false);
  const [refreshBusy, setRefreshBusy] = useState(false);
  const [revokeBusyId, setRevokeBusyId] = useState("");
  const [deleteOutboundBusyId, setDeleteOutboundBusyId] = useState("");

  const incomingRequests = useMemo(() => projectIncomingRequests(requests), [requests]);
  const trustedPeers = useMemo(() => projectTrustedPeers(trusts), [trusts]);
  const overview = useMemo(() => projectPairingOverview({ identity, requests, trusts }), [identity, requests, trusts]);
  const libp2pRegistrations = useMemo(() => filterLibp2pRegistrations(registrations), [registrations]);
  const recentOutbounds = useMemo(() => projectRecentOutbounds(outbounds), [outbounds]);
  const parsed = useMemo(() => parseConnectionInfoInput(connectionInput), [connectionInput]);
  const localPeerId = identity?.peer_id || "";
  const localNodeId = identity?.node_id || "";
  const inviteReady = canCreateInvite({ groupId: currentGroupId, busy, issuerEndpoint });
  const sameInstanceInput = isSameInstancePairingInput(parsed);
  const localEndpoint = isLocalIssuerEndpoint(issuerEndpoint);
  const requestReady = canSubmitPairingRequest({
    pairingCode: parsed.pairingCode,
    requesterGroupId: currentGroupId,
    requesterPeerId: localPeerId,
    busy,
  });

  const onCreateInvite = useCallback(async () => {
    setInviteError("");
    setCopyNotice("");
    setBusy(true);
    try {
      const normalizedEndpoint = normalizeIssuerEndpoint(issuerEndpoint);
      setIssuerEndpoint(normalizedEndpoint);
      const resp = await api.createFederationPairingInvite({ groupId: currentGroupId });
      if (resp.ok) {
        const infoResp = await api.createFederationPairingConnectionInfo({
          groupId: currentGroupId,
          inviteId: resp.result.invite.invite_id,
          issuerEndpoint: normalizedEndpoint,
          issuerGroupTitle: currentGroupTitle || currentGroupId,
        });
        if (!infoResp.ok) {
          const errorKey = userFacingPairingErrorKey(infoResp.error.message);
          setInviteError(errorKey ? t(errorKey) : (infoResp.error.message || t("federation.createInviteFailed")));
          return;
        }
        setCreatedInfo(JSON.stringify(infoResp.result.payload, null, 2));
        await refreshPairing();
      } else {
        setInviteError(resp.error.message || t("federation.createInviteFailed"));
      }
    } catch {
      setInviteError(t("federation.issuerEndpointInvalid"));
    } finally {
      setBusy(false);
    }
  }, [currentGroupId, currentGroupTitle, issuerEndpoint, refreshPairing, t]);

  const onCopyConnectionInfo = useCallback(async () => {
    if (!createdInfo) return;
    const copied = await copyTextToClipboard(createdInfo);
    setCopyNotice(t(copied ? "federation.copyConnectionInfoDone" : "federation.copyConnectionInfoManual"));
  }, [createdInfo, t]);

  const onCreateRequest = useCallback(async () => {
    setRequestError("");
    setRequestNotice("");
    setBusy(true);
    try {
      const resp = parsed.isRemote && parsed.payload
        ? await api.createFederationRemotePairingRequest({
          localGroupId: currentGroupId,
          localGroupTitle: currentGroupTitle || currentGroupId,
          payload: parsed.payload,
        })
        : await api.createFederationPairingRequest({
          pairingCode: parsed.pairingCode,
          requesterGroupId: currentGroupId,
          requesterPeerId: localPeerId,
          inviteId: parsed.nonce,
        });
      if (resp.ok) {
        setConnectionInput("");
        setRequestNotice(t("federation.waitingApproval"));
        await refreshPairing();
      } else {
        const errorKey = userFacingPairingErrorKey(resp.error.message);
        setRequestError(errorKey ? t(errorKey) : (shouldUsePairingCodeHelp(resp.error.message) ? t("federation.pairingCodeInvalidOrExpired") : (resp.error.message || t("federation.submitRequestFailed"))));
      }
    } finally {
      setBusy(false);
    }
  }, [currentGroupId, currentGroupTitle, localPeerId, parsed, refreshPairing, t]);

  const review = useCallback(async (requestId: string, action: "approve" | "reject") => {
    setReviewError("");
    setBusy(true);
    try {
      const resp = action === "approve"
        ? await api.approveFederationPairingRequest(requestId)
        : await api.rejectFederationPairingRequest(requestId);
      if (!resp.ok) setReviewError(resp.error.message || t(action === "approve" ? "federation.approveFailed" : "federation.rejectFailed"));
      await refreshPairing();
      if (resp.ok && action === "approve") publishFederationPairingChanged(currentGroupId);
    } finally {
      setBusy(false);
    }
  }, [currentGroupId, refreshPairing, t]);

  const onRefresh = useCallback(async () => {
    setRefreshBusy(true);
    try {
      await refreshPairing();
    } finally {
      setRefreshBusy(false);
    }
  }, [refreshPairing]);

  const revokeTrustedPeer = useCallback(async (trustId: string) => {
    const tid = String(trustId || "").trim();
    if (!tid) return;
    setReviewError("");
    setRevokeBusyId(tid);
    try {
      const resp = await api.revokeFederationTrust(tid);
      if (!resp.ok) setReviewError(resp.error.message || t("federation.revokeTrustFailed"));
      await refreshPairing();
      if (resp.ok) publishFederationPairingChanged(currentGroupId);
    } finally {
      setRevokeBusyId("");
    }
  }, [currentGroupId, refreshPairing, t]);

  const deleteOutbound = useCallback(async (outboundId: string) => {
    const oid = String(outboundId || "").trim();
    if (!oid) return;
    setReviewError("");
    setDeleteOutboundBusyId(oid);
    try {
      const resp = await api.deleteFederationPairingOutbound(oid);
      if (!resp.ok) setReviewError(resp.error.message || t("federation.deleteSentRequestFailed"));
      await refreshPairing();
    } finally {
      setDeleteOutboundBusyId("");
    }
  }, [refreshPairing, t]);

  return (
    <div className={settingsWorkspaceBodyClass}>
      <section className={settingsWorkspacePanelClass(isDark)}>
        <div className="flex flex-wrap items-start justify-between gap-4">
          <div className="min-w-0">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.overview")}</div>
            <h3 className="mt-1 text-lg font-semibold text-[var(--color-text-primary)]">{t("federation.connectRemoteCcccGroup")}</h3>
            <p className="mt-2 max-w-2xl text-sm leading-6 text-[var(--color-text-muted)]">
              {t("federation.groupConnectionsLead", { group: currentGroupTitle || currentGroupId || "-" })}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <span className="rounded-full border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-3 py-1 text-xs font-medium text-[var(--color-text-primary)]">{overview.identityReady ? t("federation.identityReady") : t("federation.identityPending")}</span>
            <span className="rounded-full border border-amber-300/40 bg-amber-100/70 px-3 py-1 text-xs font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-200">{t("federation.pendingCount", { count: overview.pendingCount })}</span>
            <span className="rounded-full border border-emerald-300/40 bg-emerald-100/70 px-3 py-1 text-xs font-medium text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">{t("federation.trustedCount", { count: overview.trustedCount })}</span>
          </div>
        </div>
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.createConnectionInfo")}</div>
          <p className="mt-2 text-sm leading-6 text-[var(--color-text-muted)]">{t("federation.createConnectionInfoHelp")}</p>
          <label className="mt-4 block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]" htmlFor="federation-issuer-endpoint">{t("federation.issuerEndpoint")}</label>
          <input
            id="federation-issuer-endpoint"
            className={`${inputClass(isDark)} mt-2`}
            value={issuerEndpoint}
            onChange={(event) => setIssuerEndpoint(event.target.value)}
            onBlur={() => {
              try {
                setIssuerEndpoint(normalizeIssuerEndpoint(issuerEndpoint));
              } catch {
                // Keep the user-entered value visible; create will surface the localized error.
              }
            }}
            placeholder="https://cccc.example.com"
          />
          <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
            {localEndpoint ? t("federation.issuerEndpointLocalOnlyHelp") : t("federation.issuerEndpointHelp")}
          </p>
          {inviteError && <p className="mt-3 text-xs font-medium text-rose-600 dark:text-rose-400">{inviteError}</p>}
          <button type="button" className={`mt-4 ${primaryButtonClass(busy)}`} disabled={!inviteReady} onClick={onCreateInvite}>{t("federation.createConnectionInfo")}</button>
          {createdInfo && (
            <div className="mt-4 rounded-2xl border border-emerald-300/40 bg-emerald-50/80 p-4 text-sm text-emerald-900 dark:bg-emerald-500/10 dark:text-emerald-100">
              <div className="text-xs font-semibold uppercase tracking-wide">{t("federation.oneTimeConnectionInfo")}</div>
              <pre className="mt-2 max-h-48 overflow-auto rounded-lg bg-white/70 p-3 font-mono text-xs text-emerald-950 dark:bg-black/20 dark:text-emerald-50">{createdInfo}</pre>
              <button type="button" className={`mt-3 ${secondaryButtonClass("sm")}`} onClick={onCopyConnectionInfo}>{t("federation.copyConnectionInfo")}</button>
              <p className="mt-2 text-xs leading-5">{t("federation.oneTimeConnectionInfoHelp")}</p>
              {copyNotice && <p className="mt-2 text-xs font-medium">{copyNotice}</p>}
            </div>
          )}
        </section>

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.pasteConnectionInfo")}</div>
          <p className="mt-2 text-sm leading-6 text-[var(--color-text-muted)]">{t("federation.pasteConnectionInfoHelp")}</p>
          <textarea className={`${inputClass(isDark)} mt-4 min-h-[118px] resize-y font-mono text-xs`} value={connectionInput} onChange={(event) => setConnectionInput(event.target.value)} placeholder={t("federation.remotePayloadPlaceholder")} />
          <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("federation.localPeerAutoMetadata")}</p>
          {parsed.isRemote && parsed.issuerEndpoint && (
            <div className="mt-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3 text-xs leading-5 text-[var(--color-text-muted)]">
              <div className="font-semibold text-[var(--color-text-primary)]">{t("federation.remotePayloadDetected")}</div>
              <div className="mt-1">{t("federation.remotePayloadTarget", { endpoint: parsed.issuerEndpoint, group: parsed.remoteGroupId || t("federation.unknownPeer") })}</div>
            </div>
          )}
          {sameInstanceInput && (
            <div className="mt-3 rounded-2xl border border-amber-300/40 bg-amber-50/80 px-4 py-3 text-xs leading-5 text-amber-900 dark:bg-amber-500/10 dark:text-amber-100">
              <div className="font-semibold">{t("federation.sameInstanceFallbackDetected")}</div>
              <div className="mt-1">{t("federation.sameInstanceFallbackHelp")}</div>
            </div>
          )}
          {requestError && <p className="mt-3 text-xs font-medium text-rose-600 dark:text-rose-400">{requestError}</p>}
          {requestNotice && <p className="mt-3 text-xs font-medium text-emerald-700 dark:text-emerald-300">{requestNotice}</p>}
          <button type="button" className={`mt-4 ${secondaryButtonClass("md")}`} disabled={!requestReady} onClick={onCreateRequest}>{t("federation.submitRequest")}</button>
          <details className="mt-4 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
            <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.sameInstanceFallback")}</summary>
            <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">{t("federation.sameInstanceFallbackAdvancedHelp")}</p>
          </details>
        </section>
      </div>

      <section className={settingsWorkspacePanelClass(isDark)}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.incomingRequests")}</div>
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            disabled={busy || refreshBusy}
            onClick={onRefresh}
            title={t("federation.refreshConnections")}
          >
            <RefreshIcon size={14} className={refreshBusy ? "animate-spin" : ""} />
            {refreshBusy ? t("federation.refreshingConnections") : t("federation.refreshConnections")}
          </button>
        </div>
        {reviewError && <p className="mt-3 text-xs font-medium text-rose-600 dark:text-rose-400">{reviewError}</p>}
        {incomingRequests.length === 0 ? <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("federation.nonePending")}</p> : (
          <div className="mt-3 space-y-2">{incomingRequests.map((request) => (
            <div key={request.request_id} className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">{formatPeerLabel(request, t("federation.unknownPeer"))}</div>
                <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-text-muted)]"><span className="rounded-full bg-amber-100 px-2 py-0.5 font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-200">{t("federation.pendingBadge")}</span><span>{request.request_id}</span></div>
              </div>
              <div className="flex gap-2"><button type="button" className={secondaryButtonClass("sm")} disabled={busy} onClick={() => review(request.request_id, "approve")}>{t("federation.approve")}</button><button type="button" className={dangerButtonClass("sm")} disabled={busy} onClick={() => review(request.request_id, "reject")}>{t("federation.reject")}</button></div>
            </div>
          ))}</div>
        )}
      </section>

      {recentOutbounds.length > 0 && (
        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.sentRequests")}</div>
          <div className="mt-3 space-y-2">{recentOutbounds.map((outbound) => (
            <div key={outbound.outbound_id} className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                  {outbound.issuer_peer_id || outbound.issuer_group_id || outbound.issuer_endpoint || outbound.outbound_id}
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-text-muted)]">
                  <span className="rounded-full bg-sky-100 px-2 py-0.5 font-medium text-sky-800 dark:bg-sky-500/15 dark:text-sky-200">{t("federation.status", { status: outbound.status })}</span>
                  <span>{outbound.outbound_id}</span>
                  {outbound.last_error && <span className="text-rose-600 dark:text-rose-400">{outbound.last_error}</span>}
                </div>
              </div>
              <button
                type="button"
                className={dangerButtonClass("sm")}
                disabled={busy || deleteOutboundBusyId === outbound.outbound_id}
                onClick={() => deleteOutbound(outbound.outbound_id)}
              >
                {deleteOutboundBusyId === outbound.outbound_id ? t("federation.deletingSentRequest") : t("federation.deleteSentRequest")}
              </button>
            </div>
          ))}</div>
        </section>
      )}

      <section className={settingsWorkspacePanelClass(isDark)}>
        <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.trustedRemoteGroups")}</div>
        {trustedPeers.length === 0 && libp2pRegistrations.length === 0 ? <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("federation.noneYet")}</p> : (
          <div className="mt-3 space-y-2">{trustedPeers.map((trust) => (
            <div key={trust.trust_id} className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">{formatPeerLabel(trust, t("federation.unknownPeer"))}</div>
                <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-text-muted)]">
                  <span className="rounded-full bg-emerald-100 px-2 py-0.5 font-medium text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">{t("federation.trustedBadge")}</span>
                  <span>{t("federation.remoteCccc", { endpoint: formatRemoteInstanceLabel(trust, t("federation.unknownCccc")) })}</span>
                  <span>{t("federation.status", { status: trust.status })}</span>
                </div>
              </div>
              <button
                type="button"
                className={dangerButtonClass("sm")}
                disabled={busy || revokeBusyId === trust.trust_id}
                onClick={() => revokeTrustedPeer(trust.trust_id)}
              >
                {revokeBusyId === trust.trust_id ? t("federation.revokingTrust") : t("federation.revokeTrust")}
              </button>
            </div>
          ))}</div>
        )}
      </section>

      <details className={settingsWorkspacePanelClass(isDark)}>
        <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("federation.localDiagnostics")}</summary>
        <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("federation.localDiagnosticsHelp")}</p>
        <div className="mt-3 grid gap-3 sm:grid-cols-2">
          <div className={settingsWorkspaceSoftPanelClass(isDark)}><div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("federation.nodeId")}</div><div className="mt-1 break-all text-sm text-[var(--color-text-primary)]">{localNodeId || t("federation.notLoaded")}</div></div>
          <div className={settingsWorkspaceSoftPanelClass(isDark)}><div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("federation.peerId")}</div><div className="mt-1 break-all text-sm text-[var(--color-text-primary)]">{localPeerId || t("federation.notLoaded")}</div></div>
          <div className={`${settingsWorkspaceSoftPanelClass(isDark)} sm:col-span-2`}><div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("federation.multiaddrs")}</div><div className="mt-1 break-all text-sm text-[var(--color-text-primary)]">{t("federation.localMultiaddrsUnavailable")}</div></div>
        </div>
      </details>
    </div>
  );
}

export default FederationLibp2pPairingSection;
