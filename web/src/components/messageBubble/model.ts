import type { Actor, LedgerEvent } from "../../types";
import { getRecipientDisplayName } from "../../hooks/useActorDisplayName";

export function buildToLabel({
    hasDestination,
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
        return "";
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
    displayNameMap,
}: {
    senderId: string;
    senderActor: Actor | null;
    senderTitle?: string;
    group_bridgeSourceName?: string;
    displayNameMap: Map<string, string>;
}): string {
    if (!senderId || senderId === "user") return senderId;
    const sourceName = String(group_bridgeSourceName || "").trim();
    if (senderId.startsWith("group_bridge:") && sourceName) return sourceName;
    return String(senderTitle || "").trim() || String(senderActor?.title || "").trim() || displayNameMap.get(senderId) || senderId;
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

export function computeAckSummary({
    hideDirectUserObligationSummary,
    isAttention,
    replyRequired,
    ackStatus,
    isUserMessage,
}: {
    hideDirectUserObligationSummary: boolean;
    isAttention: boolean;
    replyRequired: boolean;
    ackStatus: LedgerEvent["_ack_status"];
    isUserMessage: boolean;
}): { done: number; total: number; needsUserAck: boolean } | null {
    if (hideDirectUserObligationSummary) return null;
    if ((!isAttention && !replyRequired) || !ackStatus || typeof ackStatus !== "object") return null;
    const ids = Object.keys(ackStatus);
    if (ids.length === 0) return null;
    const done = ids.reduce((n, id) => n + (ackStatus[id] ? 1 : 0), 0);
    const needsUserAck =
        Object.prototype.hasOwnProperty.call(ackStatus, "user") && !ackStatus["user"] && !isUserMessage;
    return { done, total: ids.length, needsUserAck };
}

export function computeObligationSummary({
    hideDirectUserObligationSummary,
    obligationStatus,
}: {
    hideDirectUserObligationSummary: boolean;
    obligationStatus: LedgerEvent["_obligation_status"];
}): { kind: "reply" | "ack"; done: number; total: number } | null {
    if (hideDirectUserObligationSummary) return null;
    if (!obligationStatus || typeof obligationStatus !== "object") return null;
    const ids = Object.keys(obligationStatus);
    if (ids.length === 0) return null;

    const requiresReply = ids.some((id) => !!obligationStatus[id]?.reply_required);
    if (requiresReply) {
        const done = ids.reduce((n, id) => n + (obligationStatus[id]?.replied ? 1 : 0), 0);
        return { kind: "reply", done, total: ids.length };
    }
    const done = ids.reduce((n, id) => n + (obligationStatus[id]?.acked ? 1 : 0), 0);
    return { kind: "ack", done, total: ids.length };
}
