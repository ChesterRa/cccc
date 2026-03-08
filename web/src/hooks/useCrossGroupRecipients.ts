// useCrossGroupRecipients - Manage recipient actors for cross-group messaging
// Extracts recipientActors, recipientActorsBusy, destGroupScopeLabel state and sync logic

import { useEffect, useMemo, useState } from "react";
import * as api from "../services/api";
import type { Actor, GroupDoc } from "../types";

interface UseCrossGroupRecipientsOptions {
  /** Current group's actors */
  actors: Actor[];
  /** Current group's document */
  groupDoc: GroupDoc | null;
  /** Currently selected group ID */
  selectedGroupId: string;
  /** Group ID that the composer state currently belongs to */
  composerGroupId: string;
  /** Target group ID for sending (from useComposerStore.destGroupId) */
  sendGroupId: string;
}

interface UseCrossGroupRecipientsResult {
  /** Actors in the target (destination) group for recipient validation */
  recipientActors: Actor[];
  /** Whether recipientActors is being fetched */
  recipientActorsBusy: boolean;
  /** Label for the target group's active scope */
  destGroupScopeLabel: string;
}

function getActiveScopeLabel(doc: GroupDoc | null): string {
  if (!doc) return "";
  const key = String(doc.active_scope_key || "").trim();
  if (!key) return "";
  const scopes = Array.isArray(doc.scopes) ? doc.scopes : [];
  const hit = scopes.find((s) => String(s?.scope_key || "").trim() === key);
  const label = String(hit?.label || "").trim();
  const url = String(hit?.url || "").trim();
  return label || url;
}

export function useCrossGroupRecipients({
  actors,
  groupDoc,
  selectedGroupId,
  composerGroupId,
  sendGroupId,
}: UseCrossGroupRecipientsOptions): UseCrossGroupRecipientsResult {
  const selectedGid = String(selectedGroupId || "").trim();
  const composerGid = String(composerGroupId || "").trim();
  const sendGid = String(sendGroupId || "").trim();
  const canFetchRemoteRecipients = !!sendGid && !!selectedGid && sendGid !== selectedGid && composerGid === selectedGid;

  // Remote fetch caches (state drives re-render).
  const [remoteActorsByGroup, setRemoteActorsByGroup] = useState<Record<string, Actor[]>>({});
  const [remoteGroupDocsByGroup, setRemoteGroupDocsByGroup] = useState<Record<string, GroupDoc>>({});

  const remoteDocForSend = canFetchRemoteRecipients ? remoteGroupDocsByGroup[sendGid] : undefined;
  useEffect(() => {
    if (!canFetchRemoteRecipients) return;
    if (remoteDocForSend) return;

    let cancelled = false;
    void api.fetchGroup(sendGid).then((resp) => {
      if (cancelled) return;
      if (!resp.ok) return;
      const doc = resp.result.group;
      setRemoteGroupDocsByGroup((prev) => ({ ...prev, [sendGid]: doc }));
    });

    return () => {
      cancelled = true;
    };
  }, [canFetchRemoteRecipients, remoteDocForSend, sendGid]);

  const remoteActorsForSend = canFetchRemoteRecipients ? remoteActorsByGroup[sendGid] : undefined;
  useEffect(() => {
    if (!canFetchRemoteRecipients) return;
    if (remoteActorsForSend) return;

    let cancelled = false;
    void api.fetchActors(sendGid).then((resp) => {
      if (cancelled) return;
      if (!resp.ok) {
        setRemoteActorsByGroup((prev) => {
          if (Object.prototype.hasOwnProperty.call(prev, sendGid)) return prev;
          return { ...prev, [sendGid]: [] };
        });
        return;
      }
      const next = resp.result.actors || [];
      setRemoteActorsByGroup((prev) => ({ ...prev, [sendGid]: next }));
    });

    return () => {
      cancelled = true;
    };
  }, [canFetchRemoteRecipients, remoteActorsForSend, sendGid]);

  const destGroupScopeLabel = useMemo(() => {
    if (!sendGid) return "";
    if (sendGid === selectedGid) return getActiveScopeLabel(groupDoc);
    const doc = remoteGroupDocsByGroup[sendGid] ?? null;
    return getActiveScopeLabel(doc);
  }, [groupDoc, remoteGroupDocsByGroup, selectedGid, sendGid]);

  const recipientActors = useMemo(() => {
    if (!sendGid) return [];
    if (sendGid === selectedGid) return actors;
    return remoteActorsByGroup[sendGid] ?? [];
  }, [actors, remoteActorsByGroup, selectedGid, sendGid]);

  const recipientActorsBusy = useMemo(() => {
    if (!sendGid) return false;
    if (!selectedGid) return false;
    if (sendGid === selectedGid) return false;
    if (!canFetchRemoteRecipients) return false;
    return !Object.prototype.hasOwnProperty.call(remoteActorsByGroup, sendGid);
  }, [canFetchRemoteRecipients, remoteActorsByGroup, selectedGid, sendGid]);

  return { recipientActors, recipientActorsBusy, destGroupScopeLabel };
}
