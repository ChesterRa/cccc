// useGlobalEvents - Subscribe to global event stream for group/actor updates
// Falls back to polling after consecutive SSE errors

import { useEffect, useRef } from "react";
import * as api from "../services/api";
import { publishCapabilityChanged } from "../utils/capabilityEvents";
import { publishFederationPairingChanged } from "../utils/federationPairingEvents";

const GLOBAL_REFRESH_EVENT_KINDS = new Set([
  "group.created",
  "group.updated",
  "group.deleted",
  "group.state_changed",
  "actor.remove",
  "actor.start",
  "actor.stop",
  "actor.restart",
]);

const ACTOR_REFRESH_EVENT_KINDS = new Set([
  "actor.remove",
  "actor.start",
  "actor.stop",
  "actor.restart",
  "group.state_changed",
]);

const CAPABILITY_REFRESH_EVENT_KINDS = new Set([
  "capability.changed",
]);

const FEDERATION_PAIRING_EVENT_KINDS = new Set([
  "federation.pairing.invite_created",
  "federation.pairing.request_created",
  "federation.pairing.request_approved",
  "federation.pairing.request_rejected",
  "federation.pairing.trust_revoked",
  "federation.pairing.outbound_changed",
  "federation.pairing.outbound_approved",
]);

export function shouldRefreshGroupsAfterGlobalEventsOpen(_hasConnectedOnce: boolean): boolean {
  return true;
}

export function shouldRefreshFederationPairingAfterGlobalEventsOpen(_hasConnectedOnce: boolean): boolean {
  return true;
}

export function shouldKeepGlobalEventsConnected(documentHidden: boolean): boolean {
  return !documentHidden;
}

export function getGlobalEventGroupId(ev: unknown): string {
  if (!ev || typeof ev !== "object") return "";
  const directGroupId = String((ev as { group_id?: unknown }).group_id || "").trim();
  if (directGroupId) return directGroupId;
  const data = (ev as { data?: unknown }).data;
  if (!data || typeof data !== "object") return "";
  return String((data as { group_id?: unknown }).group_id || "").trim();
}

export function shouldRefreshActorsAfterGlobalEvent(ev: unknown, selectedGroupId: string): boolean {
  if (!ev || typeof ev !== "object") return false;
  const kind = String((ev as { kind?: unknown }).kind || "").trim();
  if (!ACTOR_REFRESH_EVENT_KINDS.has(kind)) return false;
  const selected = String(selectedGroupId || "").trim();
  if (!selected) return false;
  return getGlobalEventGroupId(ev) === selected;
}

export function shouldRefreshCapabilitiesAfterGlobalEvent(ev: unknown, selectedGroupId: string): boolean {
  if (!ev || typeof ev !== "object") return false;
  const kind = String((ev as { kind?: unknown }).kind || "").trim();
  if (!CAPABILITY_REFRESH_EVENT_KINDS.has(kind)) return false;
  const selected = String(selectedGroupId || "").trim();
  if (!selected) return false;
  return getGlobalEventGroupId(ev) === selected;
}

export function shouldRefreshFederationPairingAfterGlobalEvent(ev: unknown, selectedGroupId: string): boolean {
  if (!ev || typeof ev !== "object") return false;
  const kind = String((ev as { kind?: unknown }).kind || "").trim();
  if (!FEDERATION_PAIRING_EVENT_KINDS.has(kind)) return false;
  const selected = String(selectedGroupId || "").trim();
  if (!selected) return false;
  return getGlobalEventGroupId(ev) === selected;
}

interface UseGlobalEventsOptions {
  /** Callback to refresh groups when events are received */
  refreshGroups: () => void;
  /** Callback to refresh actors for the selected group when lifecycle changes land */
  refreshActors?: (groupId: string, opts?: { includeUnread?: boolean }) => Promise<void> | void;
  /** Currently selected group id */
  selectedGroupId?: string;
  /** Callback when effective capability state for the selected group changed */
  refreshCapabilities?: (groupId: string) => void;
}

/**
 * Subscribes to the global events stream to keep sidebar status in sync.
 * Falls back to polling after 3 consecutive SSE errors.
 */
export function useGlobalEvents({ refreshGroups, refreshActors, selectedGroupId, refreshCapabilities }: UseGlobalEventsOptions): void {
  // Use ref to avoid recreating SSE connection when refreshGroups reference changes
  const refreshGroupsRef = useRef(refreshGroups);
  const refreshActorsRef = useRef(refreshActors);
  const refreshCapabilitiesRef = useRef(refreshCapabilities);
  const selectedGroupIdRef = useRef(selectedGroupId);
  const hasConnectedOnceRef = useRef(false);
  useEffect(() => {
    refreshGroupsRef.current = refreshGroups;
  }, [refreshGroups]);
  useEffect(() => {
    refreshActorsRef.current = refreshActors;
  }, [refreshActors]);
  useEffect(() => {
    refreshCapabilitiesRef.current = refreshCapabilities;
  }, [refreshCapabilities]);
  useEffect(() => {
    selectedGroupIdRef.current = selectedGroupId;
  }, [selectedGroupId]);

  useEffect(() => {
    let es: EventSource | null = null;
    let fallbackTimer: number | null = null;
    let fallbackDelayMs = 10000;
    let errorCount = 0;

    function clearFallbackTimer() {
      if (fallbackTimer) {
        window.clearTimeout(fallbackTimer);
        fallbackTimer = null;
      }
    }

    function closeSSE() {
      if (es) {
        es.close();
        es = null;
      }
    }

    function invalidateAndRefreshGroups() {
      api.invalidateGroupsRead();
      refreshGroupsRef.current();
    }

    function refreshSelectedActors() {
      const gid = String(selectedGroupIdRef.current || "").trim();
      if (!gid || !refreshActorsRef.current) return;
      api.clearActorsReadOnlyRequest(gid);
      void refreshActorsRef.current(gid, { includeUnread: false });
    }

    function refreshSelectedCapabilities() {
      const gid = String(selectedGroupIdRef.current || "").trim();
      if (!gid) return;
      if (refreshCapabilitiesRef.current) {
        refreshCapabilitiesRef.current(gid);
        return;
      }
      publishCapabilityChanged(gid);
    }

    function refreshSelectedFederationPairing() {
      const gid = String(selectedGroupIdRef.current || "").trim();
      if (!gid) return;
      publishFederationPairingChanged(gid);
    }

    function scheduleFallbackPoll() {
      if (fallbackTimer) return;
      fallbackTimer = window.setTimeout(() => {
        fallbackTimer = null;
        if (!document.hidden) {
          invalidateAndRefreshGroups();
          // While in polling fallback, periodically attempt to restore SSE.
          // If reconnect succeeds, onopen() clears fallback polling.
          if (!es) {
            connectSSE();
          }
        }
        fallbackDelayMs = Math.min(fallbackDelayMs * 2, 60000);
        scheduleFallbackPoll();
      }, fallbackDelayMs);
    }

    function connectSSE() {
      if (!shouldKeepGlobalEventsConnected(document.hidden)) {
        closeSSE();
        clearFallbackTimer();
        return;
      }
      if (es) return;
      es = new EventSource(api.withAuthToken("/api/v1/events/stream"));
      es.addEventListener("event", (e) => {
        try {
          const ev = JSON.parse((e as MessageEvent).data || "{}");
          const kind = typeof ev?.kind === "string" ? ev.kind : "";
          if (GLOBAL_REFRESH_EVENT_KINDS.has(kind)) {
            invalidateAndRefreshGroups();
          }
          if (shouldRefreshActorsAfterGlobalEvent(ev, selectedGroupIdRef.current || "")) {
            refreshSelectedActors();
          }
          if (shouldRefreshCapabilitiesAfterGlobalEvent(ev, selectedGroupIdRef.current || "")) {
            refreshSelectedCapabilities();
          }
          if (shouldRefreshFederationPairingAfterGlobalEvent(ev, selectedGroupIdRef.current || "")) {
            refreshSelectedFederationPairing();
          }
        } catch {
          /* ignore parse errors */
        }
      });
      es.onopen = () => {
        const shouldRefresh = shouldRefreshGroupsAfterGlobalEventsOpen(hasConnectedOnceRef.current);
        const shouldRefreshFederationPairing = shouldRefreshFederationPairingAfterGlobalEventsOpen(hasConnectedOnceRef.current);
        errorCount = 0; // Reset on successful connection
        fallbackDelayMs = 10000;
        clearFallbackTimer();
        hasConnectedOnceRef.current = true;
        // Global event streams start from EOF, so both first connect and
        // reconnect need a catch-up refresh to cover the open window.
        if (shouldRefresh) {
          invalidateAndRefreshGroups();
          refreshSelectedActors();
        }
        if (shouldRefreshFederationPairing) {
          refreshSelectedFederationPairing();
        }
      };
      es.onerror = () => {
        errorCount++;
        // After 3 consecutive errors, fallback to polling
        if (errorCount >= 3 && !fallbackTimer) {
          es?.close();
          es = null;
          scheduleFallbackPoll();
        }
      };
    }

    function handleVisibilityChange() {
      if (!shouldKeepGlobalEventsConnected(document.hidden)) {
        closeSSE();
        clearFallbackTimer();
        return;
      }
      errorCount = 0;
      fallbackDelayMs = 10000;
      connectSSE();
    }

    connectSSE();
    document.addEventListener("visibilitychange", handleVisibilityChange);

    return () => {
      document.removeEventListener("visibilitychange", handleVisibilityChange);
      closeSSE();
      clearFallbackTimer();
      hasConnectedOnceRef.current = false;
    };
  }, []); // Empty deps - only run on mount/unmount
}
