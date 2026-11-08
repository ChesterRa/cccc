import type { LedgerEvent } from "../types";
import {
  getOutboxEntry,
  releaseTransferredPreviewUrls,
  transferOutboxPreviewUrls,
  useChatOutboxStore,
} from "../stores/chatOutboxStore";
import { useResourcePanelStore } from "../stores/useResourcePanelStore";

export type CanonicalOutboxReconciliation = {
  event: LedgerEvent;
  clientId: string;
  transferredPreviewUrls: string[];
};

export function reconcileCanonicalOutboxEvent(
  event: LedgerEvent,
  groupId: string,
): CanonicalOutboxReconciliation {
  if (
    String(event.kind || "").trim() !== "chat.message" ||
    String(event.by || "").trim() !== "user"
  ) {
    return { event, clientId: "", transferredPreviewUrls: [] };
  }
  const data =
    event.data && typeof event.data === "object" ? (event.data as Record<string, unknown>) : null;
  const clientId = data && typeof data.client_id === "string" ? data.client_id.trim() : "";
  if (!clientId) return { event, clientId: "", transferredPreviewUrls: [] };

  const outboxEntry = getOutboxEntry(groupId, clientId);
  const optimisticData =
    outboxEntry?.event?.data && typeof outboxEntry.event.data === "object"
      ? (outboxEntry.event.data as Record<string, unknown>)
      : null;
  const optimisticAttachments: unknown[] = Array.isArray(optimisticData?.attachments)
    ? optimisticData.attachments
    : [];
  const canonicalAttachments: unknown[] = Array.isArray(data?.attachments) ? data.attachments : [];
  if (optimisticAttachments.length === 0 || canonicalAttachments.length === 0) {
    return { event, clientId, transferredPreviewUrls: [] };
  }
  // The confirmed message shows its stored copies (messageAttachmentLinks). The local copies
  // stay valid a little longer, so a raster image already on screen switches to its stored copy
  // without a blank frame, and are released after that.
  return { event, clientId, transferredPreviewUrls: transferOutboxPreviewUrls(groupId, clientId) };
}

export function completeCanonicalOutboxReconciliation(
  groupId: string,
  reconciliation: CanonicalOutboxReconciliation,
): void {
  if (!reconciliation.clientId) return;
  // An open resource must stop using the outbox's object URL before that URL is revoked.
  // The message row may have been virtualized away, so update the resource store here rather
  // than relying on that row to render the confirmed attachment. Do not open or select a tab.
  const resources = useResourcePanelStore.getState();
  if (resources.groups[groupId]) {
    const event = reconciliation.event;
    const data =
      event.data && typeof event.data === "object" ? (event.data as Record<string, unknown>) : null;
    const attachments = data && Array.isArray(data.attachments) ? data.attachments : [];
    const eventId = String(event.id || "").trim();
    const blobGroupId = String(event.group_id || "").trim() || groupId;
    attachments.forEach((attachment, index) => {
      if (!attachment || typeof attachment !== "object") return;
      const blobPath = String((attachment as { path?: unknown }).path || "").trim();
      if (!blobPath) return;
      resources.updateAttachment(groupId, `client:${reconciliation.clientId}`, index, {
        eventId,
        sentAt: event.ts,
        blobGroupId,
        blobPath,
        localPreviewUrl: undefined,
      });
    });
  }
  useChatOutboxStore.getState().remove(groupId, reconciliation.clientId);
  releaseTransferredPreviewUrls(reconciliation.transferredPreviewUrls);
}
