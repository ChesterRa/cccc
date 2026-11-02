import { useCallback, useEffect, useRef, useState } from "react";
import { classNames } from "../../utils/classNames";
import { AttachmentIcon, CloseIcon, ImageIcon } from "../../components/Icons";
import { BodyPortal } from "../../components/ui/BodyPortal";
import {
  getComposerPreviewPosition,
  isPreviewableComposerImageFile,
} from "./ComposerFilePreview.model";

export function ComposerFilePreview({
  file,
  onRemove,
  removeLabel,
}: {
  file: File;
  onRemove: () => void;
  removeLabel: string;
}) {
  const [isPreviewOpen, setIsPreviewOpen] = useState(false);
  const [previewPosition, setPreviewPosition] = useState({ left: 12, top: 12 });
  const rootRef = useRef<HTMLDivElement | null>(null);
  const canPreviewImage = isPreviewableComposerImageFile(file);
  const [preview, setPreview] = useState<{ file: File; url: string } | null>(null);
  const [failedUrl, setFailedUrl] = useState("");
  const previewUrl = preview?.file === file ? preview.url : "";
  const showThumbnail = Boolean(previewUrl && previewUrl !== failedUrl);

  useEffect(() => {
    if (!canPreviewImage || typeof URL.createObjectURL !== "function") return;
    const url = URL.createObjectURL(file);
    setPreview({ file, url });
    return () => URL.revokeObjectURL(url);
  }, [canPreviewImage, file]);

  const updatePreviewPosition = useCallback(() => {
    const root = rootRef.current;
    if (!root || typeof window === "undefined") return;
    setPreviewPosition(
      getComposerPreviewPosition({
        anchor: root.getBoundingClientRect(),
        viewport: { width: window.innerWidth, height: window.innerHeight },
      }),
    );
  }, []);

  const openPreview = useCallback(() => {
    updatePreviewPosition();
    setIsPreviewOpen(true);
  }, [updatePreviewPosition]);

  useEffect(() => {
    if (!isPreviewOpen) return undefined;
    updatePreviewPosition();
    window.addEventListener("resize", updatePreviewPosition);
    window.addEventListener("scroll", updatePreviewPosition, true);
    return () => {
      window.removeEventListener("resize", updatePreviewPosition);
      window.removeEventListener("scroll", updatePreviewPosition, true);
    };
  }, [isPreviewOpen, updatePreviewPosition]);

  return (
    <div
      ref={rootRef}
      className={classNames(
        "group relative inline-flex max-w-full items-center gap-1.5 rounded-lg border text-xs shadow-sm transition-colors",
        showThumbnail ? "p-1" : "py-1 pl-2.5 pr-1",
        "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]",
      )}
      onMouseEnter={openPreview}
      onMouseLeave={() => setIsPreviewOpen(false)}
      onFocus={openPreview}
      onBlur={() => setIsPreviewOpen(false)}
      onKeyDown={(event) => {
        if (event.key === "Escape") setIsPreviewOpen(false);
      }}
      data-composer-file-preview
    >
      {showThumbnail ? (
        <button
          type="button"
          className="h-16 w-20 overflow-hidden rounded-md focus-visible:outline-2 focus-visible:outline-offset-2"
          onClick={openPreview}
          title={file.name}
          aria-label={file.name}
          aria-expanded={isPreviewOpen}
        >
          <img
            src={previewUrl}
            alt={file.name}
            className="h-full w-full object-contain"
            onError={() => setFailedUrl(previewUrl)}
          />
        </button>
      ) : (
        <>
          {canPreviewImage ? (
            <ImageIcon size={12} className="shrink-0 text-[var(--color-text-tertiary)]" />
          ) : (
            <AttachmentIcon size={12} className="shrink-0 text-[var(--color-text-tertiary)]" />
          )}
          <span className="truncate font-medium text-[var(--color-text-primary)]" title={file.name}>
            {file.name}
          </span>
        </>
      )}
      <button
        type="button"
        className={classNames(
          // Touch keeps its 44px target; the negative margins stop it from inflating the chip.
          "flex shrink-0 h-6 w-6 items-center justify-center rounded-full",
          showThumbnail
            ? "absolute -right-1 -top-1 border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] shadow-sm"
            : "-my-3",
          "text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
        )}
        onClick={onRemove}
        aria-label={removeLabel}
        title={removeLabel}
      >
        <CloseIcon size={14} />
      </button>
      {showThumbnail && isPreviewOpen && (
        <BodyPortal>
          <div
            className={classNames(
              "pointer-events-none fixed z-[9999] w-[224px] max-w-[calc(100vw-24px)] overflow-hidden rounded-xl border p-2 shadow-2xl",
              "border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)]",
            )}
            role="tooltip"
            style={{ left: previewPosition.left, top: previewPosition.top }}
          >
            <img
              src={previewUrl}
              alt={file.name}
              className="max-h-[176px] w-full rounded-lg object-contain"
            />
            <div className="mt-1.5 truncate px-0.5 text-xs font-medium text-[var(--color-text-secondary)]">
              {file.name}
            </div>
          </div>
        </BodyPortal>
      )}
    </div>
  );
}
