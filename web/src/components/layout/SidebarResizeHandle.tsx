import type { PointerEventHandler } from "react";
import { classNames } from "../../utils/classNames";

export function SidebarResizeHandle({
  width,
  min,
  max,
  resizing,
  label,
  onPointerDown,
}: {
  width: number;
  min: number;
  max: number;
  resizing: boolean;
  label: string;
  onPointerDown: PointerEventHandler<HTMLDivElement>;
}) {
  return (
    <div
      className="absolute inset-y-0 right-0 z-20 hidden w-4 translate-x-1/2 cursor-col-resize items-center justify-center md:flex group/resize-handle"
      onPointerDown={onPointerDown}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuemin={min}
      aria-valuemax={max}
      aria-valuenow={width}
    >
      <div
        className={classNames(
          "h-14 w-[3px] rounded-full transition-all duration-300 ease-out-expo group-hover/resize-handle:h-20 group-hover/resize-handle:w-[5px]",
          resizing
            ? "h-20 w-[5px] bg-[rgb(35,36,37)] shadow-[0_0_12px_rgba(17,24,39,0.25)] dark:bg-white dark:shadow-[0_0_12px_rgba(255,255,255,0.25)]"
            : "bg-black/10 group-hover/resize-handle:bg-black/30 group-hover/resize-handle:shadow-[0_0_8px_rgba(0,0,0,0.05)] dark:bg-white/10 dark:group-hover/resize-handle:bg-white/30 dark:group-hover/resize-handle:shadow-[0_0_8px_rgba(255,255,255,0.05)]",
        )}
      />
    </div>
  );
}
