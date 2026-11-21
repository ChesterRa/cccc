import { fetchMessageWindow } from "../../services/api";
import {
  attachmentSetKey,
  useAttachmentPreviewStore,
} from "../../stores/useAttachmentPreviewStore";
import type { AttachmentResource } from "../../stores/useResourcePanelStore";
import { messageBlobAttachments, previewItems } from "../messageBubble/attachmentPreviewItems";

/**
 * Reads the one message of an attachment that the full-screen reader shows alone, such as a tab
 * restored before its message is shown again, for the message's list to step through. No history
 * is searched. The list is kept only while the reader still shows that message and the message has
 * not given its own list in the meantime.
 */
export async function loadAttachmentList(panelGroupId: string, resource: AttachmentResource) {
  if (!resource.eventId) return;
  const response = await fetchMessageWindow(panelGroupId, resource.eventId, {
    before: 0,
    after: 0,
  }).catch(() => null);
  if (!response?.ok) return;
  const event = response.result.events.find((item) => item.id === resource.eventId);
  const data = event?.data as { attachments?: unknown } | undefined;
  const items = previewItems(
    messageBlobAttachments(data?.attachments),
    resource.blobGroupId,
    (attachment) => ({
      blobGroupId: resource.blobGroupId,
      messageKey: resource.messageKey,
      eventId: resource.eventId,
      sender: resource.sender,
      sentAt: resource.sentAt,
      index: attachment.sourceIndex,
      blobPath: attachment.path,
    }),
  );
  if (items.length < 2 || !items.some((item) => item.resource.index === resource.index)) return;
  const previews = useAttachmentPreviewStore.getState();
  const { viewing, sets } = previews;
  if (viewing?.panelGroupId !== panelGroupId || viewing.messageKey !== resource.messageKey) return;
  if ((sets[attachmentSetKey(panelGroupId, resource.messageKey)]?.length ?? 0) > 1) return;
  previews.remember(panelGroupId, resource.messageKey, items);
}
