import { useLayoutEffect, type RefObject } from "react";

interface ComposerTextareaAutoResizeOptions {
  composerRef: RefObject<HTMLTextAreaElement | null>;
  value: string;
  minHeight: number;
  maxHeight: number;
}

export function resizeComposerTextarea(
  node: HTMLTextAreaElement,
  minHeight: number,
  maxHeight: number,
): number {
  node.style.height = "auto";
  const nextHeight = Math.min(Math.max(node.scrollHeight, minHeight), maxHeight);
  node.style.height = `${nextHeight}px`;
  return nextHeight;
}

export function useComposerTextareaAutoResize({
  composerRef,
  value,
  minHeight,
  maxHeight,
}: ComposerTextareaAutoResizeOptions): void {
  useLayoutEffect(() => {
    const node = composerRef.current;
    if (!node) return;
    resizeComposerTextarea(node, minHeight, maxHeight);
  }, [composerRef, maxHeight, minHeight, value]);
}
