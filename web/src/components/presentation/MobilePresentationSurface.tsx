import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { classNames } from "../../utils/classNames";

export function MobilePresentationSurface({
  isOpen,
  isDark,
  label,
  onClose,
  children,
}: {
  isOpen: boolean;
  isDark: boolean;
  label: string;
  onClose: () => void;
  children: ReactNode;
}) {
  const surfaceRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!isOpen) return undefined;
    const previousFocus =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    window.requestAnimationFrame(() => surfaceRef.current?.focus());
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
      previousFocus?.focus();
    };
  }, [isOpen, onClose]);

  if (!isOpen || typeof document === "undefined") return null;

  return createPortal(
    <div
      ref={surfaceRef}
      className={classNames(
        "fixed inset-0 z-[45] flex min-h-0 min-w-0 flex-col overflow-hidden",
        "pt-[env(safe-area-inset-top,0px)] pr-[env(safe-area-inset-right,0px)] pb-[env(safe-area-inset-bottom,0px)] pl-[env(safe-area-inset-left,0px)]",
        isDark ? "bg-slate-950 text-slate-100" : "bg-[var(--color-bg-primary)] text-gray-900",
      )}
      role="dialog"
      aria-modal="true"
      aria-label={label}
      tabIndex={-1}
      data-mobile-presentation-surface="true"
    >
      {children}
    </div>,
    document.body,
  );
}
