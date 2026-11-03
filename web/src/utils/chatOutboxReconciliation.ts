import type { LedgerEvent } from "../types";
import {
  getOutboxEntry,
  releaseTransferredPreviewUrls,
  transferOutboxPreviewUrls,
  useChatOutboxStore,
} from "../stores/chatOutboxStore";

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
  useChatOutboxStore.getState().remove(groupId, reconciliation.clientId);
  releaseTransferredPreviewUrls(reconciliation.transferredPreviewUrls);
}
