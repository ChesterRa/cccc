import type { CSSProperties } from "react";
import { useComposerDraftViewer, useComposerFileUrls } from "./composerDraftAttachments";
import { ComposerFilePreview } from "./ComposerFilePreview";

/**
 * Files waiting to be sent, floating over the conversation on the composer's top edge. The
 * tray is out of the composer's flow, so attaching a file never changes the composer's height
 * or moves the text being typed. Only the files themselves take the pointer; the gaps between
 * them stay clickable for whatever lies underneath.
 */
export function ComposerFileTray({
  groupId,
  files,
  onRemove,
  removeLabel,
}: {
  /** The Group whose attachment viewer a clicked picture opens in. */
  groupId: string;
  files: readonly File[];
  onRemove: (index: number) => void;
  removeLabel: (file: File) => string;
}) {
  const urls = useComposerFileUrls(files);
  const enlarge = useComposerDraftViewer(groupId, files, urls);
  if (files.length === 0) return null;
  return (
    <ul data-composer-file-tray className="composer-file-tray" aria-live="polite">
      {files.map((file, index) => (
        <li
          key={`${file.name}:${index}`}
          className="composer-file-tray-item"
          // Each file leans and bobs a little differently, so a row of them does not move as
          // one block.
          style={{ "--tray-index": index } as CSSProperties}
        >
          <ComposerFilePreview
            file={file}
            previewUrl={urls.get(file)}
            onEnlarge={() => enlarge(index)}
            onRemove={() => onRemove(index)}
            removeLabel={removeLabel(file)}
          />
        </li>
      ))}
    </ul>
  );
}
