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
  remoteSourceName,
  groupLabelById = {},
  displayNameMap,
}: {
  senderId: string;
  senderActor: Actor | null;
  senderTitle?: string;
  remoteSourceName?: string;
  groupLabelById?: Record<string, string>;
  displayNameMap: Map<string, string>;
}): string {
  if (!senderId || senderId === "user") return senderId;
  const sourceName = String(remoteSourceName || "").trim();
  if ((senderId.startsWith("group_bridge:") || senderId.startsWith("connect:")) && sourceName)
    return sourceName;
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
}): { replied: number; cancelled: number; total: number } | null {
  if (hideDirectUserObligationSummary) return null;
  if (!obligationStatus || typeof obligationStatus !== "object") return null;
  const ids = Object.keys(obligationStatus).filter((id) => obligationStatus[id]?.reply_requested);
  if (ids.length === 0) return null;
  let replied = 0;
  let cancelled = 0;
  for (const id of ids) {
    // The daemon never reports both; if stale data does, count the recipient
    // once and let cancellation win, as the recipients dialog does.
    if (obligationStatus[id]?.cancelled) cancelled += 1;
    else if (obligationStatus[id]?.replied) replied += 1;
  }
  return { replied, cancelled, total: ids.length };
}
