// Drag-and-drop file handling.
import { useEffect, useRef, useState, useCallback } from "react";
import { useUIStore, useComposerStore } from "../stores";
import { useShallow } from "zustand/react/shallow";

const WEB_MAX_FILE_MB = 100;
const WEB_MAX_FILE_BYTES = WEB_MAX_FILE_MB * 1024 * 1024;

export function partitionAttachments(
  files: File[],
  existingBytes = 0,
): { accepted: File[]; rejected: File[] } {
  let acceptedBytes = Math.max(0, existingBytes);
  const accepted: File[] = [];
  const rejected: File[] = [];
  for (const file of files) {
    if (file.size > WEB_MAX_FILE_BYTES || acceptedBytes + file.size > WEB_MAX_FILE_BYTES) {
      rejected.push(file);
      continue;
    }
    acceptedBytes += file.size;
    accepted.push(file);
  }
  return { accepted, rejected };
}

interface UseDragDropOptions {
  selectedGroupId: string;
}

export function useDragDrop({ selectedGroupId }: UseDragDropOptions) {
  const { showError } = useUIStore(useShallow((s) => ({ showError: s.showError })));
  const { appendComposerFiles, composerFiles } = useComposerStore(
    useShallow((s) => ({
      appendComposerFiles: s.appendComposerFiles,
      composerFiles: s.composerFiles,
    })),
  );

  const [dropOverlayOpen, setDropOverlayOpen] = useState(false);
  const dragDepthRef = useRef<number>(0);

  // Handle adding files to the composer.
  const handleAppendComposerFiles = useCallback(
    (incoming: File[]) => {
      const files = Array.from(incoming || []);
      if (files.length === 0) return;

      const existingBytes = composerFiles.reduce(
        (total, file) => total + Math.max(0, file.size),
        0,
      );
      const { accepted: ok, rejected: tooLarge } = partitionAttachments(files, existingBytes);

      if (tooLarge.length > 0) {
        const names = tooLarge.slice(0, 3).map((f) => f.name || "file");
        const more = tooLarge.length > 3 ? ` (+${tooLarge.length - 3} more)` : "";
        showError(`Attachments exceed ${WEB_MAX_FILE_MB} MiB: ${names.join(", ")}${more}`);
      }

      if (ok.length > 0) {
        appendComposerFiles(ok);
      }
    },
    [showError, appendComposerFiles, composerFiles],
  );

  // Drag/drop event listeners.
  useEffect(() => {
    const hasFiles = (e: DragEvent) => {
      const dt = e.dataTransfer;
      if (!dt) return false;
      try {
        if (dt.types && Array.from(dt.types).includes("Files")) return true;
        if (dt.items && Array.from(dt.items).some((it) => it.kind === "file")) return true;
      } catch {
        // ignore
      }
      return dt.files && dt.files.length > 0;
    };

    const onDragEnter = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      dragDepthRef.current += 1;
      setDropOverlayOpen(true);
    };

    const onDragOver = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
    };

    const onDragLeave = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      dragDepthRef.current = Math.max(0, dragDepthRef.current - 1);
      if (dragDepthRef.current === 0) setDropOverlayOpen(false);
    };

    const onDrop = (e: DragEvent) => {
      if (!hasFiles(e)) return;
      e.preventDefault();
      dragDepthRef.current = 0;
      setDropOverlayOpen(false);

      const files = Array.from(e.dataTransfer?.files || []);
      if (files.length === 0) return;
      if (!selectedGroupId) {
        showError("Select a group to attach files.");
        return;
      }
      handleAppendComposerFiles(files);
    };

    window.addEventListener("dragenter", onDragEnter, true);
    window.addEventListener("dragover", onDragOver, true);
    window.addEventListener("dragleave", onDragLeave, true);
    window.addEventListener("drop", onDrop, true);

    return () => {
      window.removeEventListener("dragenter", onDragEnter, true);
      window.removeEventListener("dragover", onDragOver, true);
      window.removeEventListener("dragleave", onDragLeave, true);
      window.removeEventListener("drop", onDrop, true);
    };
  }, [handleAppendComposerFiles, selectedGroupId, showError]);

  // Reset drag/drop state.
  const resetDragDrop = useCallback(() => {
    dragDepthRef.current = 0;
    setDropOverlayOpen(false);
  }, []);

  return { dropOverlayOpen, handleAppendComposerFiles, resetDragDrop, WEB_MAX_FILE_MB };
}
