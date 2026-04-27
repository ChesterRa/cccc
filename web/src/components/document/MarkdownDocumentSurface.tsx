import { memo } from "react";
import { LazyMarkdownRenderer } from "../LazyMarkdownRenderer";
import { classNames } from "../../utils/classNames";

type MarkdownDocumentSurfaceProps = {
  isDark?: boolean;
  content: string;
  error?: string;
  emptyLabel?: string;
  className?: string;
  previewClassName?: string;
  minHeightClassName?: string;
  editing?: boolean;
  editValue?: string;
  editPlaceholder?: string;
  editAriaLabel?: string;
  onEditValueChange?: (value: string) => void;
};

export const MarkdownDocumentSurface = memo(function MarkdownDocumentSurface({
  isDark,
  content,
  error,
  emptyLabel,
  className,
  previewClassName,
  minHeightClassName = "min-h-[180px]",
  editing,
  editValue,
  editPlaceholder,
  editAriaLabel,
  onEditValueChange,
}: MarkdownDocumentSurfaceProps) {
  const value = String(editing ? editValue ?? content : content || "");
  const hasContent = value.trim().length > 0;

  return (
    <div
      className={classNames(
        "rounded-3xl border",
        editing ? "overflow-hidden p-0" : "p-5",
        minHeightClassName,
        isDark ? "border-white/10 bg-slate-950/60" : "border-black/10 bg-white/90",
        className,
      )}
    >
      {error ? (
        <div className={classNames("text-sm", isDark ? "text-rose-300" : "text-rose-600")}>{error}</div>
      ) : editing ? (
        <textarea
          value={value}
          onChange={(event) => onEditValueChange?.(event.target.value)}
          placeholder={editPlaceholder}
          aria-label={editAriaLabel}
          className={classNames(
            "block h-full w-full resize-y overflow-y-auto rounded-3xl border-0 bg-transparent p-5 font-mono text-[12px] leading-5 outline-none scrollbar-subtle",
            minHeightClassName,
            isDark ? "text-slate-100 placeholder:text-slate-500" : "text-gray-900 placeholder:text-gray-400",
          )}
        />
      ) : hasContent ? (
        <LazyMarkdownRenderer
          content={value}
          isDark={isDark}
          className={classNames("break-words [overflow-wrap:anywhere]", previewClassName)}
          fallback={<div className={classNames("whitespace-pre-wrap break-words", previewClassName)}>{value}</div>}
        />
      ) : (
        <div className={classNames("flex h-full items-center justify-center text-sm", isDark ? "text-slate-500" : "text-gray-500")}>
          {emptyLabel || ""}
        </div>
      )}
    </div>
  );
});
