import type { Actor, LedgerEvent } from "../../types";
import { getRecipientDisplayName } from "../../hooks/useActorDisplayName";

export function buildToLabel({
  hasDestination,
  dstTo,
  recipients,
  displayNameMap,
}: {
  hasDestination: boolean;
  dstGroupId: string;
  dstTo: string[];
  groupLabelById: Record<string, string>;
  recipients: string[] | undefined;
  displayNameMap: Map<string, string>;
}): string {
  if (hasDestination) {
    if (!dstTo || dstTo.length === 0) return "";
    return dstTo.map((recipient) => getRecipientDisplayName(recipient, displayNameMap)).join(", ");
  }
  if (!recipients || recipients.length === 0) return "@foreman";
  return recipients
    .map((recipient) => getRecipientDisplayName(recipient, displayNameMap))
    .join(", ");
}

export function getSenderDisplayName({
  senderId,
  senderActor,
  senderTitle,
  group_bridgeSourceName,
  groupLabelById = {},
  displayNameMap,
}: {
  senderId: string;
  senderActor: Actor | null;
  senderTitle?: string;
  group_bridgeSourceName?: string;
  groupLabelById?: Record<string, string>;
  displayNameMap: Map<string, string>;
}): string {
  if (!senderId || senderId === "user") return senderId;
  const sourceName = String(group_bridgeSourceName || "").trim();
  if (senderId.startsWith("group_bridge:") && sourceName) return sourceName;
  const [senderGroupId, senderActorId] = senderId.split("::", 2);
  const senderGroupLabel = String(groupLabelById[senderGroupId] || "").trim();
  if (senderGroupLabel && senderActorId) {
    const senderActorLabel =
      String(senderTitle || "").trim() || String(senderActor?.title || "").trim() || senderActorId;
    return `${senderGroupLabel}::${senderActorLabel}`;
  }
  return (
    String(senderTitle || "").trim() ||
    String(senderActor?.title || "").trim() ||
    displayNameMap.get(senderId) ||
    senderId
  );
}

export function buildVisibleReadStatusEntries(
  actors: Actor[],
  readStatus: LedgerEvent["_read_status"],
): [string, boolean][] {
  if (!readStatus) return [];
  return actors
    .map((actor) => String(actor.id || ""))
    .filter((id) => id && Object.prototype.hasOwnProperty.call(readStatus, id))
    .map((id) => [id, !!readStatus[id]] as [string, boolean]);
}

export function computeObligationSummary({
  hideDirectUserObligationSummary,
  obligationStatus,
}: {
  hideDirectUserObligationSummary: boolean;
  obligationStatus: LedgerEvent["_obligation_status"];
}): { done: number; total: number } | null {
  if (hideDirectUserObligationSummary) return null;
  if (!obligationStatus || typeof obligationStatus !== "object") return null;
  const ids = Object.keys(obligationStatus).filter((id) => obligationStatus[id]?.reply_requested);
  if (ids.length === 0) return null;
  const done = ids.reduce(
    (count, id) =>
      count + (obligationStatus[id]?.replied || obligationStatus[id]?.cancelled ? 1 : 0),
    0,
  );
  return { done, total: ids.length };
}
