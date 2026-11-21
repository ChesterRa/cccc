import {
  type AttachmentPreviewItem,
  useAttachmentPreviewStore,
} from "../../stores/useAttachmentPreviewStore";
import { type AttachmentResource, useResourcePanelStore } from "../../stores/useResourcePanelStore";
import type { AttachmentResourceMedia } from "./views/AttachmentResourceView";

/** The displayed conversation and the Group owning a blob can be different. */
export type MessageAttachmentSource = {
  panelGroupId: string;
  blobGroupId: string;
  messageKey: string;
  eventId: string;
  sender?: string;
  sentAt?: string;
  index: number;
  blobPath: string;
  localPreviewUrl?: string;
  /** The message's previewable attachments in its own order, to step through. */
  siblings: AttachmentPreviewItem[];
};

/**
 * Opens one attachment without keeping its expiring resource address in tab state. While the
 * panel is on screen it opens there, beside the messages; otherwise it is shown over the whole
 * page and the panel stays as it was.
 */
export function openAttachmentResource(
  source: MessageAttachmentSource,
  media: AttachmentResourceMedia,
  title: string,
  isSvg = false,
) {
  const { panelGroupId, siblings, ...attachment } = source;
  const resource: AttachmentResource = { kind: "attachment", ...attachment, media, isSvg };
  const panels = useResourcePanelStore.getState();
  const previews = useAttachmentPreviewStore.getState();
  if (panels.shownGroupId === panelGroupId) {
    panels.open(panelGroupId, resource, title);
    previews.remember(panelGroupId, resource.messageKey, siblings);
    return;
  }
  const listed = siblings.some((item) => item.resource.index === resource.index);
  previews.view(
    panelGroupId,
    resource.messageKey,
    listed ? siblings : [{ resource, title }],
    resource.index,
  );
}
