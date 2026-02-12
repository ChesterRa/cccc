// useGlobalEvents - Subscribe to global event stream for group/actor updates
// Falls back to polling after consecutive SSE errors

import { useEffect, useRef } from "react";
import * as api from "../services/api";

interface UseGlobalEventsOptions {
  /** Callback to refresh groups when events are received */
  refreshGroups: () => void;
}

/**
 * Subscribes to the global events stream to keep sidebar status in sync.
 * Falls back to polling after 3 consecutive SSE errors.
 */
export function useGlobalEvents({ refreshGroups }: UseGlobalEventsOptions): void {
  // Use ref to avoid recreating SSE connection when refreshGroups reference changes
  const refreshGroupsRef = useRef(refreshGroups);
  useEffect(() => {
    refreshGroupsRef.current = refreshGroups;
  }, [refreshGroups]);

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

    function scheduleFallbackPoll() {
      if (fallbackTimer) return;
      fallbackTimer = window.setTimeout(() => {
        fallbackTimer = null;
        if (!document.hidden) {
          refreshGroupsRef.current();
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
      if (es) return;
      es = new EventSource(api.withAuthToken("/api/v1/events/stream"));
      es.addEventListener("event", (e) => {
        try {
          const ev = JSON.parse((e as MessageEvent).data || "{}");
          const kind = typeof ev?.kind === "string" ? ev.kind : "";
          // Refresh groups on group or actor events to keep sidebar status in sync
          if (kind.startsWith("group.") || kind.startsWith("actor.")) {
            refreshGroupsRef.current();
          }
        } catch {
          /* ignore parse errors */
        }
      });
      es.onopen = () => {
        errorCount = 0; // Reset on successful connection
        fallbackDelayMs = 10000;
        clearFallbackTimer();
        refreshGroupsRef.current(); // Re-sync after reconnects (best-effort)
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

    connectSSE();

    return () => {
      es?.close();
      clearFallbackTimer();
    };
  }, []); // Empty deps - only run on mount/unmount
}
