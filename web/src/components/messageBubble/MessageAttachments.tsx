import type { MessageAttachment } from "../../types";
import { classNames } from "../../utils/classNames";
import { buildMessageAttachmentLinks } from "../../utils/messageAttachmentLinks";
import {
  attachmentDocumentKind,
  isAudioAttachment,
  isImageAttachment,
  isSvgAttachment,
  isVideoAttachment,
} from "../../utils/messageAttachments";
import { FileIcon } from "../Icons";
import { AudioAttachments } from "./AudioAttachments";
import { DocumentAttachment } from "./DocumentAttachment";
import { ImagePreview } from "./ImagePreview";
import { VideoPreview } from "./VideoPreview";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";

export function MessageAttachments({
  attachments,
  blobGroupId,
  isUserMessage,
  isDark,
  attachmentKeyPrefix,
  downloadTitle,
}: {
  attachments: MessageAttachment[];
  blobGroupId: string;
  isUserMessage: boolean;
  isDark: boolean;
  attachmentKeyPrefix: string;
  downloadTitle: (name: string) => string;
}) {
  const imageAttachments = attachments.filter((attachment) => isImageAttachment(attachment));
  const videoAttachments = attachments.filter((attachment) => isVideoAttachment(attachment));
  const audioAttachments = attachments.filter((attachment) => isAudioAttachment(attachment));
  const fileAttachments = attachments.filter(
    (attachment) =>
      !isImageAttachment(attachment) &&
      !isVideoAttachment(attachment) &&
      !isAudioAttachment(attachment),
  );
  const useImageGrid = imageAttachments.length > 1;

  if (attachments.length <= 0 || !blobGroupId) return null;

  return (
    <div className="mt-3" data-message-attachments>
      {imageAttachments.length > 0 && (
        <div
          className={classNames(
            "max-w-full items-start gap-2",
            useImageGrid ? "grid w-fit grid-cols-2" : "flex max-w-[min(30rem,82vw)] flex-col gap-3",
            !useImageGrid && !isUserMessage && "w-[min(30rem,82vw)]",
          )}
        >
          {imageAttachments.map((attachment, index) => {
            const links = buildMessageAttachmentLinks({
              groupId: blobGroupId,
              path: attachment.path,
              title: attachment.title,
              localPreviewUrl: attachment.local_preview_url,
              fallbackLabel: "image",
            });
            return (
              <div
                key={`img:${attachmentKeyPrefix}:${index}`}
                className={classNames(
                  "flex flex-col",
                  useImageGrid ? "w-[10rem] sm:w-[11rem]" : "w-full max-w-[min(30rem,82vw)]",
                )}
              >
                <ImagePreview
                  href={links.previewHref}
                  downloadHref={links.downloadHref}
                  downloadName={links.downloadName}
                  alt={links.label}
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
      {videoAttachments.length > 0 && (
        <div
          className={classNames(
            "flex max-w-full flex-col items-start gap-3",
            !isUserMessage && "w-[min(30rem,82vw)]",
            imageAttachments.length > 0 && "mt-3",
          )}
        >
          {videoAttachments.map((attachment, index) => {
            const links = buildMessageAttachmentLinks({
              groupId: blobGroupId,
              path: attachment.path,
              title: attachment.title,
              localPreviewUrl: attachment.local_preview_url,
              fallbackLabel: "video",
            });
            return (
              <VideoPreview
                key={`video:${attachmentKeyPrefix}:${index}`}
                href={links.previewHref}
                downloadHref={links.downloadHref}
                downloadName={links.downloadName}
                label={links.label}
                downloadTitle={downloadTitle(links.label)}
              />
            );
          })}
        </div>
      )}
      {audioAttachments.length > 0 && (
        <div
          className={classNames(
            (imageAttachments.length > 0 || videoAttachments.length > 0) && "mt-3",
          )}
        >
          <AudioAttachments
            attachments={audioAttachments}
            blobGroupId={blobGroupId}
            attachmentKeyPrefix={attachmentKeyPrefix}
            downloadTitle={downloadTitle}
          />
        </div>
      )}
      {fileAttachments.length > 0 && (
        <div
          className={classNames(
            "flex max-w-full flex-wrap items-start gap-2",
            (imageAttachments.length > 0 ||
              videoAttachments.length > 0 ||
              audioAttachments.length > 0) &&
              "mt-3",
          )}
        >
          {fileAttachments.map((attachment, index) => {
            const links = buildMessageAttachmentLinks({
              groupId: blobGroupId,
              path: attachment.path,
              title: attachment.title,
              localPreviewUrl: attachment.local_preview_url,
              fallbackLabel: "file",
            });
            const documentKind = attachmentDocumentKind(attachment);
            if (documentKind) {
              return (
                <DocumentAttachment
                  key={`file:${attachmentKeyPrefix}:${index}`}
                  kind={documentKind}
                  href={links.previewHref}
                  downloadHref={links.downloadHref}
                  downloadName={links.downloadName}
                  label={links.label}
                  downloadTitle={downloadTitle(links.label)}
                  isDark={isDark}
                />
              );
            }
            return (
              <AuthenticatedDownloadLink
                key={`file:${attachmentKeyPrefix}:${index}`}
                href={links.downloadHref}
                className={classNames(
                  "inline-flex max-w-full items-center gap-2 rounded-full px-2.5 py-1.5 text-xs transition-colors",
                  "border border-[var(--glass-border-subtle)] bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)]",
                )}
                title={downloadTitle(links.label)}
                download={links.downloadName}
              >
                <FileIcon size={13} className="opacity-60 flex-shrink-0" />
                <span
                  className="truncate font-medium"
                  style={{ color: "var(--color-text-primary)" }}
                >
                  {links.label}
                </span>
              </AuthenticatedDownloadLink>
            );
          })}
        </div>
      )}
    </div>
  );
}
