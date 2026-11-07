import { useTranslation } from "react-i18next";
import { readFrameProof } from "../../features/connect/protocol";
import type { MessageLinkPreview as Preview } from "../../utils/messageLinkPreview";
import { GraphicViewer } from "../viewer/GraphicViewer";
import { WorkspaceMediaPreview } from "../workspace/WorkspaceMediaPreview";
import { DocumentPreview } from "./DocumentPreview";
import { MediaLightbox } from "./MediaLightbox";
import { pauseOtherMessageMedia } from "./messageMedia";

/** What a link in a message points to, shown over the page and loaded only now. */
export function MessageLinkPreview({
  preview,
  isDark,
  onClose,
}: {
  preview: Preview;
  isDark: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation("chat");
  const { kind, href, label, downloadHref } = preview;
  // This site's files need the embedded cookie partition, which a new tab leaves.
  const openHref = downloadHref && readFrameProof(window.location) ? undefined : href;
  const failedText = t(openHref ? "linkPreviewFailed" : "documentPreviewFailed");
  return (
    <MediaLightbox
      open
      onClose={onClose}
      dialogLabel={t("documentPreviewDialog")}
      title={label}
      downloadHref={downloadHref}
      downloadName={label}
      openHref={openHref}
      image={kind === "image"}
    >
      {kind === "image" ? (
        <GraphicViewer src={href} alt={label} onBlankClick={onClose} />
      ) : kind === "video" || kind === "audio" ? (
        <WorkspaceMediaPreview
          src={href}
          name={label}
          audio={kind === "audio"}
          failedText={failedText}
          onPlay={pauseOtherMessageMedia}
        />
      ) : (
        <DocumentPreview
          kind={kind}
          href={href}
          title={label}
          isDark={isDark}
          failedText={failedText}
        />
      )}
    </MediaLightbox>
  );
}
