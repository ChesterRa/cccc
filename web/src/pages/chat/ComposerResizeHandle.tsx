import type { KeyboardEventHandler, PointerEventHandler, RefObject } from "react";
import { classNames } from "../../utils/classNames";

export function ComposerResizeHandle({
  handleRef,
  height,
  minimum,
  maximum,
  resizing,
  label,
  onPointerDown,
  onKeyDown,
  onReset,
}: {
  handleRef: RefObject<HTMLDivElement | null>;
  height: number;
  minimum: number;
  maximum: number;
  resizing: boolean;
  label: string;
  onPointerDown: PointerEventHandler<HTMLDivElement>;
  onKeyDown: KeyboardEventHandler<HTMLDivElement>;
  onReset: () => void;
}) {
  return (
    <div
      ref={handleRef}
      role="separator"
      aria-orientation="horizontal"
      aria-label={label}
      aria-valuemin={Math.round(minimum)}
      aria-valuemax={Math.round(maximum)}
      aria-valuenow={Math.round(height)}
      tabIndex={0}
      title={label}
      onPointerDown={onPointerDown}
      onKeyDown={onKeyDown}
      onDoubleClick={onReset}
      className="group/composer-resize absolute inset-x-0 top-0 z-20 hidden h-3 -translate-y-1/2 cursor-ns-resize items-center justify-center touch-none focus-visible:outline-2 md:flex"
    >
      <div
        className={classNames(
          "h-[3px] w-14 rounded-full transition-colors group-hover/composer-resize:h-[5px] group-hover/composer-resize:w-20 group-focus-visible/composer-resize:w-20",
          resizing
            ? "bg-[var(--color-text-primary)]"
            : "bg-[var(--glass-border)] group-hover/composer-resize:bg-[var(--color-text-secondary)]",
        )}
      />
    </div>
  );
}
