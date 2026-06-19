import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../../../services/api";
import type {
  FederationIdentity,
  FederationPairingOutbound,
  FederationPairingRequest,
  FederationRegistration,
  FederationTrust,
} from "../../../services/api/federation";
import { FederationSessionPairingSection } from "./FederationSessionPairingSection";
import { projectSyncableOutbounds } from "./federationPairingModel";
import { subscribeFederationPairingChanged } from "../../../utils/federationPairingEvents";

interface FederationConnectionsSectionProps {
  isDark: boolean;
  isActive?: boolean;
  groupId: string;
  groupTitle?: string;
}

export function FederationConnectionsSection({
  isDark,
  isActive,
  groupId,
  groupTitle,
}: FederationConnectionsSectionProps) {
  const [registrations, setRegistrations] = useState<FederationRegistration[]>([]);
  const [identity, setIdentity] = useState<FederationIdentity | null>(null);
  const [requests, setRequests] = useState<FederationPairingRequest[]>([]);
  const [trusts, setTrusts] = useState<FederationTrust[]>([]);
  const [outbounds, setOutbounds] = useState<FederationPairingOutbound[]>([]);
  const refreshInFlightRef = useRef(false);
  const refreshQueuedRef = useRef(false);

  const syncSubmittedOutbounds = useCallback(async (items: FederationPairingOutbound[]) => {
    const syncable = projectSyncableOutbounds(items);
    if (syncable.length === 0) return false;
    await Promise.allSettled(syncable.map((outbound) => api.syncFederationPairingOutbound(outbound.outbound_id)));
    return true;
  }, []);

  const refreshPairing = useCallback(async () => {
    if (!groupId) return;
    if (refreshInFlightRef.current) {
      refreshQueuedRef.current = true;
      return;
    }
    refreshInFlightRef.current = true;
    try {
      const [identityResp, initialRequestResp, initialTrustResp, initialStatusResp, initialOutboundResp] = await Promise.all([
        api.fetchFederationIdentity(),
        api.fetchFederationPairingRequests(groupId),
        api.fetchFederationTrusts(groupId),
        api.fetchFederationStatus(groupId),
        api.fetchFederationPairingOutbounds(groupId),
      ]);
      let requestResp = initialRequestResp;
      let trustResp = initialTrustResp;
      let statusResp = initialStatusResp;
      let outboundResp = initialOutboundResp;
      if (outboundResp.ok && await syncSubmittedOutbounds(outboundResp.result.outbounds || [])) {
        [requestResp, trustResp, statusResp, outboundResp] = await Promise.all([
          api.fetchFederationPairingRequests(groupId),
          api.fetchFederationTrusts(groupId),
          api.fetchFederationStatus(groupId),
          api.fetchFederationPairingOutbounds(groupId),
        ]);
      }
      if (identityResp.ok) setIdentity(identityResp.result.identity);
      if (requestResp.ok) setRequests(requestResp.result.requests || []);
      if (trustResp.ok) setTrusts(trustResp.result.trusts || []);
      if (statusResp.ok) setRegistrations(statusResp.result.registrations || []);
      if (outboundResp.ok) setOutbounds(outboundResp.result.outbounds || []);
    } finally {
      refreshInFlightRef.current = false;
      if (refreshQueuedRef.current) {
        refreshQueuedRef.current = false;
        void refreshPairing();
      }
    }
  }, [groupId, syncSubmittedOutbounds]);

  useEffect(() => {
    if (isActive === false || !groupId) return;
    const task = window.setTimeout(() => {
      void refreshPairing();
    }, 0);
    return () => window.clearTimeout(task);
  }, [groupId, isActive, refreshPairing]);

  useEffect(() => {
    if (isActive === false || !groupId) return;
    return subscribeFederationPairingChanged(groupId, () => {
      void refreshPairing();
    });
  }, [groupId, isActive, refreshPairing]);

  return (
    <FederationSessionPairingSection
      isDark={isDark}
      currentGroupId={groupId}
      currentGroupTitle={groupTitle}
      registrations={registrations}
      identity={identity}
      requests={requests}
      trusts={trusts}
      outbounds={outbounds}
      refreshPairing={refreshPairing}
    />
  );
}

export default FederationConnectionsSection;
