// 文件拖放 hook
import { useEffect, useRef, useState, useCallback } from "react";
import { useUIStore, useComposerStore } from "../stores";

const WEB_MAX_FILE_MB = 20;
const WEB_MAX_FILE_BYTES = WEB_MAX_FILE_MB * 1024 * 1024;

interface UseDragDropOptions {
  selectedGroupId: string;
}

export function useDragDrop({ selectedGroupId }: UseDragDropOptions) {
  const { showError } = useUIStore();
  const { appendComposerFiles } = useComposerStore();

  const [dropOverlayOpen, setDropOverlayOpen] = useState(false);
  const dragDepthRef = useRef<number>(0);

  // 处理添加文件
  const handleAppendComposerFiles = useCallback(
    (incoming: File[]) => {
      const files = Array.from(incoming || []);
      if (files.length === 0) return;

      const tooLarge = files.filter((f) => f.size > WEB_MAX_FILE_BYTES);
      const ok = files.filter((f) => f.size <= WEB_MAX_FILE_BYTES);

      if (tooLarge.length > 0) {
        const names = tooLarge.slice(0, 3).map((f) => f.name || "file");
        const more = tooLarge.length > 3 ? ` (+${tooLarge.length - 3} more)` : "";
        showError(`File too large (> ${WEB_MAX_FILE_MB}MB): ${names.join(", ")}${more}`);
      }

      if (ok.length > 0) {
        appendComposerFiles(ok);
      }
    },
    [showError, appendComposerFiles]
  );

  // 拖放事件监听
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

  // 重置拖放状态
  const resetDragDrop = useCallback(() => {
    dragDepthRef.current = 0;
    setDropOverlayOpen(false);
  }, []);

  return {
    dropOverlayOpen,
    handleAppendComposerFiles,
    resetDragDrop,
    WEB_MAX_FILE_MB,
  };
}
