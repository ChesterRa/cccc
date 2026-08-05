import { useEffect } from "react";
import * as api from "../services/api";
import { useRuntimeActivityStore } from "../stores";
import type { RuntimeActivityEvent } from "../types";

export function parseRuntimeActivityEvent(value: unknown): RuntimeActivityEvent | null {
  if (!value || typeof value !== "object") return null;
  const event = value as Partial<RuntimeActivityEvent>;
  if (
    !String(event.id || "").trim() ||
    !String(event.group_id || "").trim() ||
    !String(event.actor_id || "").trim() ||
    !String(event.activity_id || "").trim() ||
    !String(event.status || "").trim()
  ) {
    return null;
  }
  return event as RuntimeActivityEvent;
}

export function useRuntimeActivityStream(groupId: string): void {
  const ingest = useRuntimeActivityStore((state) => state.ingest);
  const prune = useRuntimeActivityStore((state) => state.prune);
  const clearGroup = useRuntimeActivityStore((state) => state.clearGroup);

  useEffect(() => {
    const gid = String(groupId || "").trim();
    if (!gid) return undefined;
    let disposed = false;
    const eventSource = new EventSource(
      api.withAuthToken(api.runtimeActivityStreamPath(gid, true)),
    );
    const ingestEvent = (value: unknown) => {
      const event = parseRuntimeActivityEvent(value);
      if (event && event.group_id === gid) ingest(gid, [event]);
    };
    eventSource.addEventListener("runtime-activity", (event) => {
      if (disposed) return;
      try {
        ingestEvent(JSON.parse(String((event as MessageEvent).data || "{}")));
      } catch {
        // Malformed activity events are isolated from the live connection.
      }
    });
    void api
      .fetchRuntimeActivitySnapshot(gid, { noCache: true })
      .then((response) => {
        if (disposed || !response.ok) return;
        const events = Array.isArray(response.result.events)
          ? response.result.events.map(parseRuntimeActivityEvent).filter(Boolean)
          : [];
        ingest(gid, events as RuntimeActivityEvent[]);
      })
      .catch(() => {
        // EventSource replay remains the recovery path when hydration fails.
      });
    const pruneTimer = window.setInterval(() => prune(Date.now()), 1_000);
    return () => {
      disposed = true;
      window.clearInterval(pruneTimer);
      eventSource.close();
      clearGroup(gid);
    };
  }, [groupId, ingest, prune, clearGroup]);
}
