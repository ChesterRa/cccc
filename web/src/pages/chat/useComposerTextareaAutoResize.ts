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
  if (minHeight === maxHeight) {
    const height = `${minHeight}px`;
    if (node.style.height !== height) node.style.height = height;
    return minHeight;
  }
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
  const measuredValue = minHeight === maxHeight ? "" : value;
  useLayoutEffect(() => {
    const node = composerRef.current;
    if (!node) return;
    resizeComposerTextarea(node, minHeight, maxHeight);
  }, [composerRef, maxHeight, minHeight, measuredValue]);
}
