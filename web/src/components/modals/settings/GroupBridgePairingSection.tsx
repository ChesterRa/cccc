import { useCallback, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";

import { RefreshIcon } from "../../Icons";
import * as api from "../../../services/api";
import type { GroupBridgeIdentity, GroupBridgePairingOutbound, GroupBridgePairingRequest, GroupBridgeRegistration, GroupBridgeTrust, GroupBridgeAccessLevel } from "../../../services/api/groupBridge";
import {
  canCreateInvite,
  canSubmitPairingRequest,
  filterGroupBridgeSessionRegistrations,
  formatPeerLabel,
  formatRemoteGroupLabel,
  GROUP_BRIDGE_ACCESS_LEVELS,
  isLocalIssuerEndpoint,
  isSameInstancePairingInput,
  isSessionConnectionInfoInput,
  normalizeGroupBridgeAccessLevel,
  normalizeIssuerEndpoint,
  parseConnectionInfoInput,
  projectIncomingRequests,
  projectPairingOverview,
  projectRecentOutbounds,
  projectTrustedPeers,
  shouldUsePairingCodeHelp,
  userFacingPairingErrorKey,
} from "./groupBridgePairingModel";
import {
  dangerButtonClass,
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";
import { publishGroupBridgePairingChanged } from "../../../utils/groupBridgePairingEvents";
import { copyTextToClipboard } from "../../../utils/copy";
import { formatRecipientIdentifier } from "../../../utils/recipientIdentifier";

interface Props {
  isDark: boolean;
  currentGroupId: string;
  currentGroupTitle?: string;
  registrations: GroupBridgeRegistration[];
  identity: GroupBridgeIdentity | null;
  requests: GroupBridgePairingRequest[];
  trusts: GroupBridgeTrust[];
  outbounds: GroupBridgePairingOutbound[];
  refreshPairing: () => Promise<void>;
}

function defaultIssuerEndpoint(): string {
  return typeof window !== "undefined" ? window.location.origin : "";
}

function accessButtonClass(level: GroupBridgeAccessLevel, selected: boolean): string {
  const selectedClass = level === "full"
    ? "border-rose-500/40 bg-rose-500/15 text-rose-700 dark:text-rose-200"
    : level === "read"
      ? "border-sky-500/35 bg-sky-500/15 text-sky-700 dark:text-sky-200"
      : "border-slate-500/25 bg-[var(--color-bg-primary)] text-[var(--color-text-primary)]";
  return [
    "min-h-[32px] rounded-lg border px-2.5 py-1.5 text-xs font-semibold transition-all duration-150",
    "focus:outline-none focus:ring-2 focus:ring-slate-500/15",
    "disabled:cursor-not-allowed disabled:opacity-50",
    selected
      ? selectedClass
      : "border-transparent text-[var(--color-text-muted)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
  ].join(" ");
}

export function GroupBridgePairingSection({
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
  const [trustCopyNotice, setTrustCopyNotice] = useState("");
  const [requestNotice, setRequestNotice] = useState("");
  const [issuerEndpoint, setIssuerEndpoint] = useState(defaultIssuerEndpoint);
  const [busy, setBusy] = useState(false);
  const [refreshBusy, setRefreshBusy] = useState(false);
  const [revokeBusyId, setRevokeBusyId] = useState("");
  const [deleteOutboundBusyId, setDeleteOutboundBusyId] = useState("");
  const [accessBusyTrustId, setAccessBusyTrustId] = useState("");
  const [remoteRefreshBusyTrustId, setRemoteRefreshBusyTrustId] = useState("");
  const [remoteRefreshErrors, setRemoteRefreshErrors] = useState<Record<string, string>>({});

  const incomingRequests = useMemo(() => projectIncomingRequests(requests), [requests]);
  const trustedPeers = useMemo(() => projectTrustedPeers(trusts), [trusts]);
  const overview = useMemo(() => projectPairingOverview({ identity, requests, trusts }), [identity, requests, trusts]);
  const sessionRegistrations = useMemo(() => filterGroupBridgeSessionRegistrations(registrations), [registrations]);
  const recentOutbounds = useMemo(() => projectRecentOutbounds(outbounds), [outbounds]);
  const parsed = useMemo(() => parseConnectionInfoInput(connectionInput), [connectionInput]);
  const localPeerId = identity?.peer_id || "";
  const localNodeId = identity?.node_id || "";
  const inviteReady = canCreateInvite({ groupId: currentGroupId, busy, issuerEndpoint });
  const sameInstanceInput = isSameInstancePairingInput(parsed);
  const sessionConnectionInfoInput = isSessionConnectionInfoInput(parsed);
  const localEndpoint = isLocalIssuerEndpoint(issuerEndpoint);
  const requestReady = canSubmitPairingRequest({
    pairingCode: parsed.pairingCode,
    requesterGroupId: currentGroupId,
    requesterPeerId: localPeerId,
    isRemote: parsed.isRemote,
    busy,
  });

  const onCreateInvite = useCallback(async () => {
    setInviteError("");
    setCopyNotice("");
    setBusy(true);
    try {
      const normalizedEndpoint = normalizeIssuerEndpoint(issuerEndpoint);
      setIssuerEndpoint(normalizedEndpoint);
      const resp = await api.createGroupBridgePairingInvite({ groupId: currentGroupId });
      if (resp.ok) {
        const infoResp = await api.createGroupBridgePairingConnectionInfo({
          groupId: currentGroupId,
          inviteId: resp.result.invite.invite_id,
          issuerEndpoint: normalizedEndpoint,
          issuerGroupTitle: currentGroupTitle || currentGroupId,
        });
        if (!infoResp.ok) {
          const errorKey = userFacingPairingErrorKey(infoResp.error.message);
          setInviteError(errorKey ? t(errorKey) : (infoResp.error.message || t("group_bridge.createInviteFailed")));
          return;
        }
        setCreatedInfo(JSON.stringify(infoResp.result.payload, null, 2));
        await refreshPairing();
      } else {
        setInviteError(resp.error.message || t("group_bridge.createInviteFailed"));
      }
    } catch {
      setInviteError(t("group_bridge.issuerEndpointInvalid"));
    } finally {
      setBusy(false);
    }
  }, [currentGroupId, currentGroupTitle, issuerEndpoint, refreshPairing, t]);

  const onCopyConnectionInfo = useCallback(async () => {
    if (!createdInfo) return;
    const copied = await copyTextToClipboard(createdInfo);
    setCopyNotice(t(copied ? "group_bridge.copyConnectionInfoDone" : "group_bridge.copyConnectionInfoManual"));
  }, [createdInfo, t]);

  const copyTrustValue = useCallback(async (value: string, label: string) => {
    const text = String(value || "").trim();
    if (!text) return;
    const copied = await copyTextToClipboard(text);
    setTrustCopyNotice(copied
      ? t("group_bridge.copyFieldDone", { field: label, defaultValue: "{{field}} copied." })
      : t("group_bridge.copyFieldManual", { field: label, defaultValue: "Copy is unavailable; select {{field}} manually." }));
  }, [t]);

  const copyTrustRecipientIdentifier = useCallback(async (trust: GroupBridgeTrust, displayName: string, accessLevel: GroupBridgeAccessLevel) => {
    const remoteGroupId = String(trust.remote_group_id || "").trim();
    if (!remoteGroupId) return;
    const copied = await copyTextToClipboard(formatRecipientIdentifier({
      kind: "remote_group",
      label: displayName,
      id: remoteGroupId,
      accessLevel,
    }));
    setTrustCopyNotice(copied
      ? t("group_bridge.copyRecipientIdentifierDone", { defaultValue: "Recipient identifier copied." })
      : t("group_bridge.copyRecipientIdentifierManual", { defaultValue: "Copy is unavailable; select the recipient identifier manually." }));
  }, [t]);

  const onCreateRequest = useCallback(async () => {
    setRequestError("");
    setRequestNotice("");
    setBusy(true);
    try {
      const resp = parsed.isRemote && parsed.payload
        ? await api.createGroupBridgeRemotePairingRequest({
          localGroupId: currentGroupId,
          localGroupTitle: currentGroupTitle || currentGroupId,
          payload: parsed.payload,
        })
        : await api.createGroupBridgePairingRequest({
          pairingCode: parsed.pairingCode,
          requesterGroupId: currentGroupId,
          requesterPeerId: localPeerId,
          inviteId: parsed.nonce,
        });
      if (resp.ok) {
        setConnectionInput("");
        setRequestNotice(t("group_bridge.waitingApproval"));
        await refreshPairing();
      } else {
        const errorKey = userFacingPairingErrorKey(resp.error.message);
        setRequestError(errorKey ? t(errorKey) : (shouldUsePairingCodeHelp(resp.error.message) ? t("group_bridge.pairingCodeInvalidOrExpired") : (resp.error.message || t("group_bridge.submitRequestFailed"))));
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
        ? await api.approveGroupBridgePairingRequest(requestId)
        : await api.rejectGroupBridgePairingRequest(requestId);
      if (!resp.ok) setReviewError(resp.error.message || t(action === "approve" ? "group_bridge.approveFailed" : "group_bridge.rejectFailed"));
      await refreshPairing();
      if (resp.ok && action === "approve") publishGroupBridgePairingChanged(currentGroupId);
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
      const resp = await api.revokeGroupBridgeTrust(tid);
      if (!resp.ok) setReviewError(resp.error.message || t("group_bridge.revokeTrustFailed"));
      await refreshPairing();
      if (resp.ok) publishGroupBridgePairingChanged(currentGroupId);
    } finally {
      setRevokeBusyId("");
    }
  }, [currentGroupId, refreshPairing, t]);

  const updateTrustAccess = useCallback(async (trustId: string, accessLevel: GroupBridgeAccessLevel) => {
    const tid = String(trustId || "").trim();
    if (!tid) return;
    setReviewError("");
    setAccessBusyTrustId(tid);
    try {
      const resp = await api.updateGroupBridgeTrustAccess(tid, accessLevel);
      if (!resp.ok) setReviewError(resp.error.message || t("group_bridge.updateAccessFailed"));
      await refreshPairing();
      if (resp.ok) publishGroupBridgePairingChanged(currentGroupId);
    } finally {
      setAccessBusyTrustId("");
    }
  }, [currentGroupId, refreshPairing, t]);

  const refreshTrustRemoteInfo = useCallback(async (trustId: string) => {
    const tid = String(trustId || "").trim();
    if (!tid) return;
    setReviewError("");
    setRemoteRefreshErrors((prev) => ({ ...prev, [tid]: "" }));
    setRemoteRefreshBusyTrustId(tid);
    try {
      const resp = await api.refreshGroupBridgeTrustRemoteInfo(tid);
      if (!resp.ok) {
        setRemoteRefreshErrors((prev) => ({
          ...prev,
          [tid]: resp.error.message || t("group_bridge.refreshRemoteInfoFailed", { defaultValue: "Could not refresh remote info." }),
        }));
        return;
      }
      await refreshPairing();
      publishGroupBridgePairingChanged(currentGroupId);
    } finally {
      setRemoteRefreshBusyTrustId("");
    }
  }, [currentGroupId, refreshPairing, t]);

  const deleteOutbound = useCallback(async (outboundId: string) => {
    const oid = String(outboundId || "").trim();
    if (!oid) return;
    setReviewError("");
    setDeleteOutboundBusyId(oid);
    try {
      const resp = await api.deleteGroupBridgePairingOutbound(oid);
      if (!resp.ok) setReviewError(resp.error.message || t("group_bridge.deleteSentRequestFailed"));
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
            <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.overview")}</div>
            <h3 className="mt-1 text-lg font-semibold text-[var(--color-text-primary)]">{t("group_bridge.connectRemoteCcccGroup")}</h3>
            <p className="mt-2 max-w-2xl text-sm leading-6 text-[var(--color-text-muted)]">
              {t("group_bridge.groupConnectionsLead", { group: currentGroupTitle || currentGroupId || "-" })}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <span className="rounded-full border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-3 py-1 text-xs font-medium text-[var(--color-text-primary)]">{overview.identityReady ? t("group_bridge.identityReady") : t("group_bridge.identityPending")}</span>
            <span className="rounded-full border border-amber-300/40 bg-amber-100/70 px-3 py-1 text-xs font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-200">{t("group_bridge.pendingCount", { count: overview.pendingCount })}</span>
            <span className="rounded-full border border-emerald-300/40 bg-emerald-100/70 px-3 py-1 text-xs font-medium text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">{t("group_bridge.trustedCount", { count: overview.trustedCount })}</span>
          </div>
        </div>
      </section>

      <div className="grid gap-4 lg:grid-cols-2">
        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.createConnectionInfo")}</div>
          <p className="mt-2 text-sm leading-6 text-[var(--color-text-muted)]">{t("group_bridge.createConnectionInfoHelp")}</p>
          <label className="mt-4 block text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]" htmlFor="group_bridge-issuer-endpoint">{t("group_bridge.issuerEndpoint")}</label>
          <input
            id="group_bridge-issuer-endpoint"
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
            {localEndpoint ? t("group_bridge.issuerEndpointLocalOnlyHelp") : t("group_bridge.issuerEndpointHelp")}
          </p>
          {inviteError && <p className="mt-3 text-xs font-medium text-rose-600 dark:text-rose-400">{inviteError}</p>}
          <button type="button" className={`mt-4 ${primaryButtonClass(busy)}`} disabled={!inviteReady} onClick={onCreateInvite}>{t("group_bridge.createConnectionInfo")}</button>
          {createdInfo && (
            <div className="mt-4 rounded-2xl border border-emerald-300/40 bg-emerald-50/80 p-4 text-sm text-emerald-900 dark:bg-emerald-500/10 dark:text-emerald-100">
              <div className="text-xs font-semibold uppercase tracking-wide">{t("group_bridge.oneTimeConnectionInfo")}</div>
              <pre className="mt-2 max-h-48 overflow-auto rounded-lg bg-white/70 p-3 font-mono text-xs text-emerald-950 dark:bg-black/20 dark:text-emerald-50">{createdInfo}</pre>
              <button type="button" className={`mt-3 ${secondaryButtonClass("sm")}`} onClick={onCopyConnectionInfo}>{t("group_bridge.copyConnectionInfo")}</button>
              <p className="mt-2 text-xs leading-5">{t("group_bridge.oneTimeConnectionInfoHelp")}</p>
              {copyNotice && <p className="mt-2 text-xs font-medium">{copyNotice}</p>}
            </div>
          )}
        </section>

        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.pasteConnectionInfo")}</div>
          <p className="mt-2 text-sm leading-6 text-[var(--color-text-muted)]">{t("group_bridge.pasteConnectionInfoHelp")}</p>
          <textarea className={`${inputClass(isDark)} mt-4 min-h-[118px] resize-y font-mono text-xs`} value={connectionInput} onChange={(event) => setConnectionInput(event.target.value)} placeholder={t("group_bridge.remotePayloadPlaceholder")} />
          <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("group_bridge.localPeerAutoMetadata")}</p>
          {parsed.isRemote && parsed.issuerEndpoint && (
            <div className="mt-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3 text-xs leading-5 text-[var(--color-text-muted)]">
              <div className="font-semibold text-[var(--color-text-primary)]">{t("group_bridge.remotePayloadDetected")}</div>
              <div className="mt-1">{t("group_bridge.remotePayloadTarget", { endpoint: parsed.issuerEndpoint, group: parsed.remoteGroupId || t("group_bridge.unknownPeer") })}</div>
            </div>
          )}
          {sameInstanceInput && !sessionConnectionInfoInput && (
            <div className="mt-3 rounded-2xl border border-amber-300/40 bg-amber-50/80 px-4 py-3 text-xs leading-5 text-amber-900 dark:bg-amber-500/10 dark:text-amber-100">
              <div className="font-semibold">{t("group_bridge.sameInstanceFallbackDetected")}</div>
              <div className="mt-1">{t("group_bridge.sameInstanceFallbackHelp")}</div>
            </div>
          )}
          {requestError && <p className="mt-3 text-xs font-medium text-rose-600 dark:text-rose-400">{requestError}</p>}
          {requestNotice && <p className="mt-3 text-xs font-medium text-emerald-700 dark:text-emerald-300">{requestNotice}</p>}
          <button type="button" className={`mt-4 ${secondaryButtonClass("md")}`} disabled={!requestReady} onClick={onCreateRequest}>{t("group_bridge.submitRequest")}</button>
        </section>
      </div>

      <section className={settingsWorkspacePanelClass(isDark)}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.incomingRequests")}</div>
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            disabled={busy || refreshBusy}
            onClick={onRefresh}
            title={t("group_bridge.refreshConnections")}
          >
            <RefreshIcon size={14} className={refreshBusy ? "animate-spin" : ""} />
            {refreshBusy ? t("group_bridge.refreshingConnections") : t("group_bridge.refreshConnections")}
          </button>
        </div>
        {reviewError && <p className="mt-3 text-xs font-medium text-rose-600 dark:text-rose-400">{reviewError}</p>}
        {incomingRequests.length === 0 ? <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("group_bridge.nonePending")}</p> : (
          <div className="mt-3 space-y-2">{incomingRequests.map((request) => (
            <div key={request.request_id} className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">{formatPeerLabel(request, t("group_bridge.unknownPeer"))}</div>
                <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-text-muted)]"><span className="rounded-full bg-amber-100 px-2 py-0.5 font-medium text-amber-800 dark:bg-amber-500/15 dark:text-amber-200">{t("group_bridge.pendingBadge")}</span><span>{request.request_id}</span></div>
              </div>
              <div className="flex gap-2"><button type="button" className={secondaryButtonClass("sm")} disabled={busy} onClick={() => review(request.request_id, "approve")}>{t("group_bridge.approve")}</button><button type="button" className={dangerButtonClass("sm")} disabled={busy} onClick={() => review(request.request_id, "reject")}>{t("group_bridge.reject")}</button></div>
            </div>
          ))}</div>
        )}
      </section>

      {recentOutbounds.length > 0 && (
        <section className={settingsWorkspacePanelClass(isDark)}>
          <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.sentRequests")}</div>
          <div className="mt-3 space-y-2">{recentOutbounds.map((outbound) => (
            <div key={outbound.outbound_id} className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
              <div className="min-w-0">
                <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">
                  {outbound.issuer_peer_id || outbound.issuer_group_id || outbound.issuer_endpoint || outbound.outbound_id}
                </div>
                <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-text-muted)]">
                  <span className="rounded-full bg-sky-100 px-2 py-0.5 font-medium text-sky-800 dark:bg-sky-500/15 dark:text-sky-200">{t("group_bridge.status", { status: outbound.status })}</span>
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
                {deleteOutboundBusyId === outbound.outbound_id ? t("group_bridge.deletingSentRequest") : t("group_bridge.deleteSentRequest")}
              </button>
            </div>
          ))}</div>
        </section>
      )}

      <section className={settingsWorkspacePanelClass(isDark)}>
        <div className="text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.trustedRemoteGroups")}</div>
        <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">{t("group_bridge.trustedRemoteGroupsHelp")}</p>
        <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">{t("group_bridge.runtimeRestartHint")}</p>
        {trustCopyNotice && <p className="mt-3 text-xs font-medium text-emerald-700 dark:text-emerald-300">{trustCopyNotice}</p>}
        {trustedPeers.length === 0 && sessionRegistrations.length === 0 ? <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("group_bridge.noneYet")}</p> : (
          <div className="mt-3 space-y-2">{trustedPeers.map((trust) => {
            const remoteGroupLabel = formatRemoteGroupLabel(trust, t("group_bridge.unknownRemoteGroup", { defaultValue: "Unknown remote group" }));
            const remoteGroupId = String(trust.remote_group_id || "").trim();
            const currentAccessLevel = normalizeGroupBridgeAccessLevel(trust.access_level);
            const remoteAccessLevel = normalizeGroupBridgeAccessLevel(trust.remote_access_level);
            const remoteAccessKnown = String(trust.remote_access_level || "").trim().length > 0;
            const refreshError = remoteRefreshErrors[trust.trust_id] || "";
            const remoteRefreshBusy = remoteRefreshBusyTrustId === trust.trust_id;
            return (
              <div key={trust.trust_id} className="rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-4 py-3">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="min-w-0">
                    <div className="truncate text-sm font-medium text-[var(--color-text-primary)]">{remoteGroupLabel}</div>
                    <div className="mt-1 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-text-muted)]">
                      <span className="rounded-full bg-emerald-100 px-2 py-0.5 font-medium text-emerald-800 dark:bg-emerald-500/15 dark:text-emerald-200">{t("group_bridge.trustedBadge")}</span>
                      <span>{t("group_bridge.status", { status: trust.status })}</span>
                    </div>
                  </div>
                  <button
                    type="button"
                    className={dangerButtonClass("sm")}
                    disabled={busy || revokeBusyId === trust.trust_id}
                    onClick={() => revokeTrustedPeer(trust.trust_id)}
                  >
                    {revokeBusyId === trust.trust_id ? t("group_bridge.revokingTrust") : t("group_bridge.revokeTrust")}
                  </button>
                </div>

                <div className="mt-3">
                  <div className={settingsWorkspaceSoftPanelClass(isDark)}>
                    <div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("group_bridge.remoteGroupId")}</div>
                    <div className="mt-1 flex items-center gap-2">
                      <code className="min-w-0 flex-1 truncate text-xs text-[var(--color-text-primary)]">{remoteGroupId || t("group_bridge.notLoaded")}</code>
                    </div>
                    {remoteGroupId && (
                      <div className="mt-2 flex flex-wrap gap-2">
                        <button
                          type="button"
                          className={secondaryButtonClass("sm")}
                          onClick={() => copyTrustValue(remoteGroupId, t("group_bridge.remoteGroupId"))}
                        >
                          {t("group_bridge.copyShort", { defaultValue: "Copy" })}
                        </button>
                        <button
                          type="button"
                          className={secondaryButtonClass("sm")}
                          onClick={() => copyTrustRecipientIdentifier(trust, remoteGroupLabel, remoteAccessLevel)}
                        >
                          {t("group_bridge.copyRecipientIdentifier", { defaultValue: "Copy identifier" })}
                        </button>
                      </div>
                    )}
                    <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
                      {t("group_bridge.remoteGroupIdAgentHelp", { defaultValue: "Copy the identifier when asking an agent to message or inspect this remote group." })}
                    </p>
                  </div>
                </div>

                <div className="mt-3 border-t border-[var(--glass-border-subtle)] pt-3">
                  <div className="grid gap-3 lg:grid-cols-2">
                    <div>
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div className="min-w-[200px] flex-1">
                          <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.accessToThisGroup")}</div>
                          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">{t("group_bridge.groupBridgeAccessHelp")}</p>
                        </div>
                        <div className="inline-flex rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-1">
                          {GROUP_BRIDGE_ACCESS_LEVELS.map((level) => {
                            const selected = currentAccessLevel === level;
                            return (
                              <button
                                key={level}
                                type="button"
                                className={accessButtonClass(level, selected)}
                                aria-pressed={selected}
                                disabled={busy || accessBusyTrustId === trust.trust_id}
                                onClick={() => {
                                  if (!selected) updateTrustAccess(trust.trust_id, level);
                                }}
                              >
                                {t(`group_bridge.accessLevels.${level}`)}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                      <p className="mt-2 text-xs leading-5 text-[var(--color-text-muted)]">
                        {t(`group_bridge.accessDescriptions.${currentAccessLevel}`)}
                      </p>
                    </div>

                    <div>
                      <div className="flex flex-wrap items-center justify-between gap-3">
                        <div className="min-w-[200px] flex-1">
                          <div className="text-[11px] font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.accessOnRemoteGroup")}</div>
                          <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">{t("group_bridge.accessOnRemoteGroupHelp")}</p>
                        </div>
                        <button
                          type="button"
                          className={secondaryButtonClass("sm")}
                          disabled={busy || remoteRefreshBusy}
                          onClick={() => refreshTrustRemoteInfo(trust.trust_id)}
                        >
                          <RefreshIcon size={14} className={remoteRefreshBusy ? "animate-spin" : ""} />
                          {remoteRefreshBusy ? t("group_bridge.refreshingConnections") : t("group_bridge.refreshRemoteInfo")}
                        </button>
                      </div>
                      <div className="mt-2 inline-flex rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-1">
                        {GROUP_BRIDGE_ACCESS_LEVELS.map((level) => {
                          const selected = remoteAccessKnown && remoteAccessLevel === level;
                          return (
                            <span key={level} className={accessButtonClass(level, selected)}>
                              {t(`group_bridge.accessLevels.${level}`)}
                            </span>
                          );
                        })}
                        {!remoteAccessKnown && (
                          <span className="min-h-[32px] rounded-lg px-2.5 py-1.5 text-xs font-semibold text-[var(--color-text-muted)]">
                            {t("group_bridge.unknownAccess", { defaultValue: "Unknown" })}
                          </span>
                        )}
                      </div>
                      {refreshError && (
                        <p className="mt-2 text-xs font-medium text-rose-600 dark:text-rose-400">
                          {refreshError}
                        </p>
                      )}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}</div>
        )}
      </section>

      <details className={settingsWorkspacePanelClass(isDark)}>
        <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wide text-[var(--color-text-muted)]">{t("group_bridge.localDiagnostics")}</summary>
        <p className="mt-3 text-xs text-[var(--color-text-muted)]">{t("group_bridge.localDiagnosticsHelp")}</p>
        <div className="mt-3 grid gap-3 sm:grid-cols-2">
          <div className={settingsWorkspaceSoftPanelClass(isDark)}><div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("group_bridge.nodeId")}</div><div className="mt-1 break-all text-sm text-[var(--color-text-primary)]">{localNodeId || t("group_bridge.notLoaded")}</div></div>
          <div className={settingsWorkspaceSoftPanelClass(isDark)}><div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("group_bridge.peerId")}</div><div className="mt-1 break-all text-sm text-[var(--color-text-primary)]">{localPeerId || t("group_bridge.notLoaded")}</div></div>
          <div className={`${settingsWorkspaceSoftPanelClass(isDark)} sm:col-span-2`}><div className="text-[11px] font-semibold uppercase text-[var(--color-text-muted)]">{t("group_bridge.sessionTransport")}</div><div className="mt-1 break-all text-sm text-[var(--color-text-primary)]">{t("group_bridge.sessionTransportManaged")}</div></div>
        </div>
      </details>
    </div>
  );
}

export default GroupBridgePairingSection;
