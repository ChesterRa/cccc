import { useTranslation } from "react-i18next";
import type { DocumentPreviewKind } from "../../utils/messageAttachments";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { DownloadIcon, FileIcon } from "../Icons";
import {
  openAttachmentResource,
  type MessageAttachmentSource,
} from "../resourcePanel/openAttachmentResource";

/** A file that opens in the page on a click, with its download beside it. */
export function DocumentAttachment({
  resource,
  kind,
  downloadHref,
  downloadName,
  label,
  downloadTitle,
}: {
  resource: MessageAttachmentSource;
  kind: DocumentPreviewKind;
  downloadHref: string;
  downloadName: string;
  label: string;
  downloadTitle: string;
}) {
  const { t } = useTranslation("chat");
  return (
    <div
      className="inline-flex max-w-full items-stretch rounded-full border border-[var(--glass-border-subtle)] text-xs"
      data-message-document
    >
      <button
        type="button"
        onClick={() => openAttachmentResource(resource, kind, label)}
        title={t("previewFile", { name: label })}
        aria-label={t("previewFile", { name: label })}
        className="inline-flex min-h-10 min-w-0 items-center gap-2 rounded-l-full py-1.5 pl-2.5 pr-1.5 text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg)]"
      >
        <FileIcon size={13} className="flex-shrink-0 opacity-60" />
        <span className="truncate font-medium" style={{ color: "var(--color-text-primary)" }}>
          {label}
        </span>
      </button>
      <AuthenticatedDownloadLink
        href={downloadHref}
        download={downloadName}
        title={downloadTitle}
        aria-label={downloadTitle}
        className="inline-flex min-h-10 min-w-10 flex-shrink-0 items-center justify-center rounded-r-full pr-1 text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg)]"
      >
        <DownloadIcon size={13} />
      </AuthenticatedDownloadLink>
    </div>
  );
}
