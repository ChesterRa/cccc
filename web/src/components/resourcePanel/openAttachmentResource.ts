import { useResourcePanelStore } from "../../stores/useResourcePanelStore";
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
};

/** Open one attachment without keeping its expiring resource address in tab state. */
export function openAttachmentResource(
  source: MessageAttachmentSource,
  media: AttachmentResourceMedia,
  title: string,
  isSvg = false,
) {
  const { panelGroupId, ...attachment } = source;
  useResourcePanelStore
    .getState()
    .open(panelGroupId, { kind: "attachment", ...attachment, media, isSvg }, title);
}
