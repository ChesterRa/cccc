import { useEffect, useMemo, useRef, useState } from "react";
import { previewItems } from "../../components/messageBubble/attachmentPreviewItems";
import {
  COMPOSER_DRAFT_MESSAGE_KEY,
  type AttachmentPreviewItem,
  useAttachmentPreviewStore,
} from "../../stores/useAttachmentPreviewStore";
import { isPreviewableComposerImageFile } from "./ComposerFilePreview.model";

/** A local address for each picture waiting to be sent, kept only while the file is attached. */
export function useComposerFileUrls(files: readonly File[]): ReadonlyMap<File, string> {
  const held = useRef(new Map<File, string>());
  const [urls, setUrls] = useState<ReadonlyMap<File, string>>(() => new Map());
  useEffect(() => {
    if (typeof URL.createObjectURL !== "function") return;
    const current = held.current;
    const next = new Map<File, string>();
    for (const file of files) {
      if (!isPreviewableComposerImageFile(file)) continue;
      next.set(file, current.get(file) ?? URL.createObjectURL(file));
    }
    for (const [file, url] of current) if (!next.has(file)) URL.revokeObjectURL(url);
    held.current = next;
    setUrls(next);
  }, [files]);
  // Whatever is still held when the composer goes away is released with it.
  useEffect(
    () => () => {
      for (const url of held.current.values()) URL.revokeObjectURL(url);
      held.current = new Map();
    },
    [],
  );
  return urls;
}

/**
 * The pictures waiting to be sent as the page's attachment viewer knows attachments, so they
 * open in the same full-size viewer as a sent picture and step through one another there.
 */
export function composerDraftItems(
  groupId: string,
  files: readonly File[],
  urls: ReadonlyMap<File, string>,
): AttachmentPreviewItem[] {
  const pictures = files.flatMap((file, sourceIndex) => {
    const url = urls.get(file);
    return url
      ? [
          {
            kind: "image",
            path: "",
            title: file.name,
            mime_type: file.type,
            local_preview_url: url,
            sourceIndex,
          },
        ]
      : [];
  });
  return previewItems(pictures, groupId, (picture) => ({
    messageKey: COMPOSER_DRAFT_MESSAGE_KEY,
    eventId: "",
    index: picture.sourceIndex ?? 0,
    blobGroupId: groupId,
    blobPath: "",
    localPreviewUrl: picture.local_preview_url,
  }));
}

/**
 * Opens a waiting picture in the attachment viewer and keeps the viewer's list in step with
 * the draft: a picture removed or sent while the viewer is open leaves it, and the viewer
 * closes when the picture it shows is gone.
 */
export function useComposerDraftViewer(
  groupId: string,
  files: readonly File[],
  urls: ReadonlyMap<File, string>,
) {
  const items = useMemo(() => composerDraftItems(groupId, files, urls), [files, groupId, urls]);
  const viewingIndex = useAttachmentPreviewStore((state) =>
    state.viewing?.panelGroupId === groupId &&
    state.viewing.messageKey === COMPOSER_DRAFT_MESSAGE_KEY
      ? state.viewing.index
      : null,
  );
  useEffect(() => {
    if (viewingIndex === null) return;
    const store = useAttachmentPreviewStore.getState();
    if (items.some((item) => item.resource.index === viewingIndex))
      store.remember(groupId, COMPOSER_DRAFT_MESSAGE_KEY, items);
    else store.close();
  }, [groupId, items, viewingIndex]);
  return (index: number) => {
    if (!groupId || !items.some((item) => item.resource.index === index)) return;
    useAttachmentPreviewStore.getState().view(groupId, COMPOSER_DRAFT_MESSAGE_KEY, items, index);
  };
}
