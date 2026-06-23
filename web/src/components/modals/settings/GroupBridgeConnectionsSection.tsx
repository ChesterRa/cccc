import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../../../services/api";
import type {
  GroupBridgeIdentity,
  GroupBridgePairingOutbound,
  GroupBridgePairingRequest,
  GroupBridgeTrust,
} from "../../../services/api/groupBridge";
import { GroupBridgePairingSection } from "./GroupBridgePairingSection";
import { projectSyncableOutbounds } from "./groupBridgePairingModel";
import { subscribeGroupBridgePairingChanged } from "../../../utils/groupBridgePairingEvents";

interface GroupBridgeConnectionsSectionProps {
  isDark: boolean;
  isActive?: boolean;
  groupId: string;
  groupTitle?: string;
}

export function GroupBridgeConnectionsSection({
  isDark,
  isActive,
  groupId,
  groupTitle,
}: GroupBridgeConnectionsSectionProps) {
  const [identity, setIdentity] = useState<GroupBridgeIdentity | null>(null);
  const [requests, setRequests] = useState<GroupBridgePairingRequest[]>([]);
  const [trusts, setTrusts] = useState<GroupBridgeTrust[]>([]);
  const [outbounds, setOutbounds] = useState<GroupBridgePairingOutbound[]>([]);
  const refreshInFlightRef = useRef(false);
  const refreshQueuedRef = useRef(false);

  const syncSubmittedOutbounds = useCallback(async (items: GroupBridgePairingOutbound[]) => {
    const syncable = projectSyncableOutbounds(items);
    if (syncable.length === 0) return false;
    await Promise.allSettled(syncable.map((outbound) => api.syncGroupBridgePairingOutbound(outbound.outbound_id)));
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
      const [identityResp, initialRequestResp, initialTrustResp, initialOutboundResp] = await Promise.all([
        api.fetchGroupBridgeIdentity(),
        api.fetchGroupBridgePairingRequests(groupId),
        api.fetchGroupBridgeTrusts(groupId),
        api.fetchGroupBridgePairingOutbounds(groupId),
      ]);
      let requestResp = initialRequestResp;
      let trustResp = initialTrustResp;
      let outboundResp = initialOutboundResp;
      if (outboundResp.ok && await syncSubmittedOutbounds(outboundResp.result.outbounds || [])) {
        [requestResp, trustResp, outboundResp] = await Promise.all([
          api.fetchGroupBridgePairingRequests(groupId),
          api.fetchGroupBridgeTrusts(groupId),
          api.fetchGroupBridgePairingOutbounds(groupId),
        ]);
      }
      if (identityResp.ok) setIdentity(identityResp.result.identity);
      if (requestResp.ok) setRequests(requestResp.result.requests || []);
      if (trustResp.ok) setTrusts(trustResp.result.trusts || []);
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
    return subscribeGroupBridgePairingChanged(groupId, () => {
      void refreshPairing();
    });
  }, [groupId, isActive, refreshPairing]);

  return (
    <GroupBridgePairingSection
      isDark={isDark}
      currentGroupId={groupId}
      currentGroupTitle={groupTitle}
      identity={identity}
      requests={requests}
      trusts={trusts}
      outbounds={outbounds}
      refreshPairing={refreshPairing}
    />
  );
}

export default GroupBridgeConnectionsSection;
