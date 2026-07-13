import type { MessageAttachment } from "../../types";
import { withAuthToken } from "../../services/api/base";
import { classNames } from "../../utils/classNames";
import { isImageAttachment, isSvgAttachment } from "../../utils/messageAttachments";
import { FileIcon } from "../Icons";
import { ImagePreview } from "./ImagePreview";

export function MessageAttachments({
  attachments,
  blobGroupId,
  isUserMessage,
  isDark,
  attachmentKeyPrefix,
  downloadTitle,
  sectionClassName,
}: {
  attachments: MessageAttachment[];
  blobGroupId: string;
  isUserMessage: boolean;
  isDark: boolean;
  attachmentKeyPrefix: string;
  downloadTitle: (name: string) => string;
  sectionClassName?: string;
}) {
  const imageAttachments = attachments.filter((attachment) => isImageAttachment(attachment));
  const fileAttachments = attachments.filter((attachment) => !isImageAttachment(attachment));
  const useImageGrid = imageAttachments.length > 1;

  if (attachments.length <= 0 || !blobGroupId) return null;

  return (
    <div className={sectionClassName || "mt-3"}>
      {imageAttachments.length > 0 && (
        <div
          className={classNames(
            "max-w-full items-start gap-2",
            useImageGrid ? "grid w-fit grid-cols-2" : "flex max-w-[min(30rem,82vw)] flex-col gap-3",
          )}
        >
          {imageAttachments.map((attachment, index) => {
            const parts = String(attachment.path || "").split("/");
            const blobName = parts[parts.length - 1] || "";
            const href =
              attachment.local_preview_url ||
              withAuthToken(
                `/api/v1/groups/${encodeURIComponent(blobGroupId)}/blobs/${encodeURIComponent(blobName)}`,
              );
            const label = attachment.title || blobName || "image";
            return (
              <div
                key={`img:${attachmentKeyPrefix}:${index}`}
                className={classNames(
                  "flex flex-col",
                  useImageGrid ? "w-[10rem] sm:w-[11rem]" : "w-full max-w-[min(30rem,82vw)]",
                )}
              >
                <ImagePreview
                  href={href}
                  alt={label}
                  isSvg={isSvgAttachment(attachment)}
                  isUserMessage={isUserMessage}
                  isDark={isDark}
                  layout={useImageGrid ? "grid" : "hero"}
                />
              </div>
            );
          })}
        </div>
      )}
      {fileAttachments.length > 0 && (
        <div
          className={classNames(
            "flex max-w-full flex-wrap items-start gap-2",
            imageAttachments.length > 0 && "mt-3",
          )}
        >
          {fileAttachments.map((attachment, index) => {
            const parts = String(attachment.path || "").split("/");
            const blobName = parts[parts.length - 1] || "";
            const href =
              attachment.local_preview_url ||
              withAuthToken(
                `/api/v1/groups/${encodeURIComponent(blobGroupId)}/blobs/${encodeURIComponent(blobName)}`,
              );
            const label = attachment.title || blobName || "file";
            return (
              <a
                key={`file:${attachmentKeyPrefix}:${index}`}
                href={href}
                className={classNames(
                  "inline-flex max-w-full items-center gap-2 rounded-full px-2.5 py-1.5 text-[11px] transition-colors",
                  "border border-[var(--glass-border-subtle)] bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)]",
                )}
                title={downloadTitle(label)}
                download
              >
                <FileIcon size={13} className="opacity-60 flex-shrink-0" />
                <span
                  className="truncate font-medium"
                  style={{ color: "var(--color-text-primary)" }}
                >
                  {label}
                </span>
              </a>
            );
          })}
        </div>
      )}
    </div>
  );
}
