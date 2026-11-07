import { useState } from "react";
import { useTranslation } from "react-i18next";
import { readFrameProof } from "../../features/connect/protocol";
import type { DocumentPreviewKind } from "../../utils/messageAttachments";
import { AuthenticatedDownloadLink } from "../AuthenticatedDownloadLink";
import { DownloadIcon, FileIcon } from "../Icons";
import { DocumentPreview } from "./DocumentPreview";
import { MediaLightbox } from "./MediaLightbox";

/** A file that opens in the page on a click, with its download beside it. */
export function DocumentAttachment({
  kind,
  href,
  downloadHref,
  downloadName,
  label,
  downloadTitle,
  isDark,
}: {
  kind: DocumentPreviewKind;
  href: string;
  downloadHref: string;
  downloadName: string;
  label: string;
  downloadTitle: string;
  isDark: boolean;
}) {
  const { t } = useTranslation("chat");
  const [open, setOpen] = useState(false);
  // A new tab leaves the embedded cookie partition, so it would not be signed in.
  const openHref = kind === "pdf" && !readFrameProof(window.location) ? href : undefined;
  return (
    <div
      className="inline-flex max-w-full items-stretch rounded-full border border-[var(--glass-border-subtle)] text-xs"
      data-message-document
    >
      <button
        type="button"
        onClick={() => setOpen(true)}
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
      <MediaLightbox
        open={open}
        onClose={() => setOpen(false)}
        dialogLabel={t("documentPreviewDialog")}
        title={label}
        downloadHref={downloadHref}
        downloadName={downloadName}
        openHref={openHref}
      >
        {open ? (
          <DocumentPreview
            kind={kind}
            href={href}
            title={label}
            isDark={isDark}
            failedText={t("documentPreviewFailed")}
          />
        ) : null}
      </MediaLightbox>
    </div>
  );
}
