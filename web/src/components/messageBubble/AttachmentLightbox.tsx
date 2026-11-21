import { AppWindow } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { readFrameProof } from "../../features/connect/protocol";
import {
  attachmentSequence,
  attachmentSetKey,
  useAttachmentPreviewStore,
} from "../../stores/useAttachmentPreviewStore";
import { resourceKey, useResourcePanelStore } from "../../stores/useResourcePanelStore";
import { buildMessageAttachmentLinks } from "../../utils/messageAttachmentLinks";
import { LcdIcon } from "../Icons";
import { AttachmentResourceView } from "../resourcePanel/views/AttachmentResourceView";
import { dialogIconKey } from "../ui/toolbarKey";
import { MediaLightbox } from "./MediaLightbox";

/**
 * A message attachment over the whole page, while the resource panel is not on screen. It steps
 * through the message's other attachments and can move the one shown into the panel.
 */
export function AttachmentLightbox({
  groupId,
  isDark,
  canOpenInPanel,
}: {
  groupId: string;
  isDark: boolean;
  /** On a phone the panel would cover the messages as this does, so it offers nothing more. */
  canOpenInPanel: boolean;
}) {
  const { t } = useTranslation("chat");
  const viewing = useAttachmentPreviewStore((state) => state.viewing);
  const items = useAttachmentPreviewStore((state) =>
    state.viewing
      ? state.sets[attachmentSetKey(state.viewing.panelGroupId, state.viewing.messageKey)]
      : undefined,
  );
  const step = useAttachmentPreviewStore((state) => state.step);
  const close = useAttachmentPreviewStore((state) => state.close);
  const [actions, setActions] = useState<HTMLSpanElement | null>(null);

  // Another conversation has its own attachments; a preview does not follow there.
  useEffect(() => {
    if (viewing && viewing.panelGroupId !== groupId) close();
  }, [close, groupId, viewing]);

  const item =
    viewing?.panelGroupId === groupId
      ? items?.find((entry) => entry.resource.index === viewing.index)
      : undefined;
  if (!viewing || !item) return null;
  const { resource, title } = item;
  const sequence = attachmentSequence(items, resource.index);
  const links = buildMessageAttachmentLinks({
    groupId: resource.blobGroupId,
    path: resource.blobPath,
    title,
    localPreviewUrl: resource.localPreviewUrl,
    fallbackLabel: resource.media,
  });
  const openInPanel = () => {
    useResourcePanelStore.getState().open(viewing.panelGroupId, resource, title);
    useAttachmentPreviewStore
      .getState()
      .remember(viewing.panelGroupId, resource.messageKey, items ?? [item]);
    close();
  };
  return (
    <MediaLightbox
      open
      onClose={close}
      dialogLabel={t("documentPreviewDialog")}
      title={title}
      downloadHref={links.downloadHref}
      downloadName={links.downloadName}
      // As in the panel: a PDF can open in the browser's own viewer, except inside a Connect
      // frame, where a new tab would leave the frame's authorization behind.
      openHref={
        resource.media === "pdf" && !readFrameProof(window.location) ? links.previewHref : undefined
      }
      image={resource.media === "image"}
      actionsRef={setActions}
      step={
        sequence
          ? {
              position: sequence.position,
              count: sequence.count,
              media: resource.media,
              onStep: step,
            }
          : undefined
      }
      leadingActions={
        canOpenInPanel ? (
          <button
            type="button"
            className={dialogIconKey}
            onClick={openInPanel}
            title={t("attachmentOpenInPanel")}
            aria-label={t("attachmentOpenInPanel")}
            data-attachment-open-in-panel
            data-lcd-key
          >
            <LcdIcon icon={AppWindow} glyph="app-windows" className="h-[18px] w-[18px]" />
          </button>
        ) : null
      }
    >
      <AttachmentResourceView
        key={resourceKey(resource)}
        media={resource.media}
        href={links.previewHref}
        title={title}
        isSvg={resource.isSvg}
        isDark={isDark}
        // Only the attachment that was clicked starts playing by itself.
        active={!viewing.stepped}
        visible
        resourceKey={resourceKey(resource)}
        actionsTarget={actions}
        onBlankClick={resource.media === "image" ? close : undefined}
        toggleButtonClassName={dialogIconKey}
        toggleIconClassName="h-[18px] w-[18px]"
      />
    </MediaLightbox>
  );
}
