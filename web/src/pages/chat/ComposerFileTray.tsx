import { useComposerDraftViewer, useComposerFileUrls } from "./composerDraftAttachments";
import { ComposerFilePreview } from "./ComposerFilePreview";

/**
 * Files waiting to be sent, laid out in-flow inside the composer so they never cover messages,
 * banners, or the dock above it. Waiting pictures still open in the Group's shared attachment
 * viewer when clicked.
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
    <ul
      data-composer-file-tray
      className="mb-2 flex max-h-40 list-none flex-wrap gap-2 overflow-y-auto px-2 pt-2 pb-1"
      aria-live="polite"
    >
      {files.map((file, index) => (
        <li key={`${file.name}:${index}`} className="max-w-full">
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
