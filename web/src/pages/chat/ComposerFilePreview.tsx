import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { classNames } from "../../utils/classNames";
import { AttachmentIcon, CloseIcon, ImageIcon } from "../../components/Icons";
import { BodyPortal } from "../../components/ui/BodyPortal";
import { getComposerPreviewPosition, isPreviewableComposerImageFile } from "./ComposerFilePreview.model";

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
  const previewUrl = useMemo(() => {
    if (!canPreviewImage || typeof URL === "undefined" || typeof URL.createObjectURL !== "function") return "";
    return URL.createObjectURL(file);
  }, [canPreviewImage, file]);

  useEffect(() => {
    return () => {
      if (previewUrl && typeof URL !== "undefined" && typeof URL.revokeObjectURL === "function") {
        URL.revokeObjectURL(previewUrl);
      }
    };
  }, [previewUrl]);

  const updatePreviewPosition = useCallback(() => {
    const root = rootRef.current;
    if (!root || typeof window === "undefined") return;
    setPreviewPosition(getComposerPreviewPosition({
      anchor: root.getBoundingClientRect(),
      viewport: { width: window.innerWidth, height: window.innerHeight },
    }));
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
        "group inline-flex max-w-full items-center gap-2 rounded-xl border px-3 py-1.5 text-xs shadow-sm transition-all",
        "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]",
      )}
      onMouseEnter={openPreview}
      onMouseLeave={() => setIsPreviewOpen(false)}
      onFocus={openPreview}
      onBlur={() => setIsPreviewOpen(false)}
    >
      {canPreviewImage ? (
        <ImageIcon size={12} className="flex-shrink-0 text-[var(--color-text-tertiary)]" />
      ) : (
        <AttachmentIcon size={12} className="flex-shrink-0 text-[var(--color-text-tertiary)]" />
      )}
      <span className="truncate font-medium text-[var(--color-text-primary)]" title={file.name}>
        {file.name}
      </span>
      <button
        className={classNames(
          "flex-shrink-0 p-1.5 -mr-1 rounded-full",
          "text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]",
        )}
        onClick={onRemove}
        aria-label={removeLabel}
        title={removeLabel}
      >
        <CloseIcon size={14} />
      </button>
      {canPreviewImage && previewUrl && isPreviewOpen && (
        <BodyPortal>
          <div
            className={classNames(
              "pointer-events-none fixed z-[9999] w-56 overflow-hidden rounded-xl border p-2 shadow-2xl",
              "border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)]",
            )}
            role="tooltip"
            style={{ left: previewPosition.left, top: previewPosition.top }}
          >
            <img
              src={previewUrl}
              alt={file.name}
              className="max-h-44 w-full rounded-lg object-contain"
            />
            <div className="mt-1.5 truncate px-0.5 text-[11px] font-medium text-[var(--color-text-secondary)]">
              {file.name}
            </div>
          </div>
        </BodyPortal>
      )}
    </div>
  );
}
