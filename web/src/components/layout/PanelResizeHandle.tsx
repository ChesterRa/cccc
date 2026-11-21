import type { HTMLAttributes } from "react";
import { classNames } from "../../utils/classNames";

type PanelResizeHandleProps = HTMLAttributes<HTMLDivElement> & {
  isDark: boolean;
  dragging: boolean;
};

export function PanelResizeHandle({
  isDark,
  dragging,
  className,
  ...props
}: PanelResizeHandleProps) {
  return (
    <div
      {...props}
      role="separator"
      tabIndex={0}
      aria-orientation="vertical"
      data-resizing={dragging || undefined}
      className={classNames(
        "group/resize relative w-2 shrink-0 touch-none cursor-col-resize",
        className,
      )}
    >
      <div className="absolute inset-y-0 left-1/2 w-px -translate-x-1/2 bg-[var(--color-border-primary)] group-hover/resize:bg-[var(--color-border-focus)] group-focus-visible/resize:bg-[var(--color-border-focus)]" />
      <div
        data-resize-band
        className={classNames(
          "absolute inset-y-0 -left-1 w-4 rounded-full transition-colors",
          dragging
            ? isDark
              ? "bg-cyan-300/18"
              : "bg-cyan-500/16"
            : isDark
              ? "hover:bg-white/8"
              : "hover:bg-black/6",
        )}
      />
    </div>
  );
}
