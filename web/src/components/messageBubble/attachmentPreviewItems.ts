import type { AttachmentPreviewItem } from "../../stores/useAttachmentPreviewStore";
import type { AttachmentResource } from "../../stores/resourcePanelModel";
import type { MessageAttachment } from "../../types";
import { buildMessageAttachmentLinks } from "../../utils/messageAttachmentLinks";
import {
  attachmentDocumentKind,
  isImageAttachment,
  isSvgAttachment,
  isVideoAttachment,
} from "../../utils/messageAttachments";
import type { AttachmentResourceMedia } from "../resourcePanel/views/AttachmentResourceView";

/** An attachment with its place among all of its message's attachments. */
export type PlacedAttachment = MessageAttachment & { sourceIndex?: number };

/**
 * The attachments a message carries as stored blobs, or as uploads still showing their local
 * preview, each keeping its place in the message even after other entries are left out.
 */
export function messageBlobAttachments(raw: unknown) {
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((a: unknown, sourceIndex) => {
    if (a == null || typeof a !== "object") return [];
    const entry = a as Record<string, unknown>;
    const attachment = {
      kind: String(entry.kind || "file"),
      path: String(entry.path || ""),
      title: String(entry.title || ""),
      bytes: Number(entry.bytes || 0),
      mime_type: String(entry.mime_type || ""),
      local_preview_url: "local_preview_url" in entry ? String(entry.local_preview_url || "") : "",
      sourceIndex,
    };
    return attachment.path.startsWith("state/blobs/") ||
      attachment.local_preview_url.startsWith("blob:")
      ? [attachment]
      : [];
  });
}

/** How the page shows an attachment, or null for audio, which plays in place, and other files. */
export function previewMedia(attachment: MessageAttachment): AttachmentResourceMedia | null {
  if (isImageAttachment(attachment)) return "image";
  if (isVideoAttachment(attachment)) return "video";
  return attachmentDocumentKind(attachment);
}

/** Where an attachment lives and which message it belongs to. */
export type AttachmentPlace = Omit<AttachmentResource, "kind" | "media" | "isSvg">;

/**
 * The attachments the page can show, in the message's own order rather than the order of the
 * message's sections, under the names the message gives them.
 */
export function previewItems<T extends PlacedAttachment>(
  attachments: T[],
  blobGroupId: string,
  where: (attachment: T) => AttachmentPlace,
): AttachmentPreviewItem[] {
  return attachments.flatMap((attachment): AttachmentPreviewItem[] => {
    const media = previewMedia(attachment);
    if (!media) return [];
    const { label } = buildMessageAttachmentLinks({
      groupId: blobGroupId,
      path: attachment.path,
      title: attachment.title,
      localPreviewUrl: attachment.local_preview_url,
      fallbackLabel: media === "image" || media === "video" ? media : "file",
    });
    return [
      {
        resource: {
          kind: "attachment",
          ...where(attachment),
          media,
          isSvg: media === "image" && isSvgAttachment(attachment),
        },
        title: label,
      },
    ];
  });
}
