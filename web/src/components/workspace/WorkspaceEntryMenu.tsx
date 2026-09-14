import { useEffect, useRef } from "react";
import { classNames } from "../../utils/classNames";

export type WorkspaceMenuItem = {
  key: string;
  label: string;
  onSelect: () => void;
  disabled?: boolean;
};

type Props = {
  x: number;
  y: number;
  items: WorkspaceMenuItem[];
  isDark: boolean;
  onClose: () => void;
};

/** Cursor-anchored menu for a tree row, kept inside the viewport. */
export function WorkspaceEntryMenu({ x, y, items, isDark, onClose }: Props) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const dismiss = (event: Event) => {
      if (!ref.current?.contains(event.target as Node)) onClose();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("pointerdown", dismiss);
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("resize", onClose);
    return () => {
      window.removeEventListener("pointerdown", dismiss);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("resize", onClose);
    };
  }, [onClose]);

  const width = 208;
  const height = items.length * 32 + 12;
  const left = Math.max(8, Math.min(x, window.innerWidth - width - 8));
  const top = Math.max(8, Math.min(y, window.innerHeight - height - 8));

  return (
    <div
      ref={ref}
      role="menu"
      style={{ left, top, width }}
      className={classNames(
        "fixed z-50 overflow-hidden rounded-xl border py-1.5 shadow-lg backdrop-blur",
        isDark ? "border-white/10 bg-slate-900/95" : "border-black/10 bg-white/95",
      )}
    >
      {items.map((item) => (
        <button
          key={item.key}
          type="button"
          role="menuitem"
          disabled={item.disabled}
          onClick={() => {
            item.onSelect();
            onClose();
          }}
          className={classNames(
            "flex w-full items-center px-3 py-1.5 text-left text-[13px] transition-colors",
            item.disabled
              ? "cursor-not-allowed opacity-40"
              : isDark
                ? "text-slate-100 hover:bg-white/8"
                : "text-slate-800 hover:bg-black/5",
          )}
        >
          {item.label}
        </button>
      ))}
    </div>
  );
}
