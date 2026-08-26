import type { ReactNode } from "react";
import { createPortal } from "react-dom";
import { useModalA11y } from "../../hooks/useModalA11y";
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
  const { modalRef } = useModalA11y(isOpen, onClose);

  if (!isOpen || typeof document === "undefined") return null;

  return createPortal(
    <div
      ref={modalRef}
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
