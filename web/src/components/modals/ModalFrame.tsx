import type { ReactNode, Ref } from "react";

interface ModalFrameProps {
  isOpen?: boolean;
  isDark: boolean;
  onClose: () => void;
  titleId: string;
  title: ReactNode;
  closeAriaLabel: string;
  panelClassName: string;
  headerActions?: ReactNode;
  footerActions?: ReactNode;
  floatingCloseClassName?: string;
  floatingCloseButtonClassName?: string;
  modalRef?: Ref<HTMLDivElement>;
  children: ReactNode;
}

export function ModalFrame({
  isOpen = true,
  isDark,
  onClose,
  titleId,
  title,
  closeAriaLabel,
  panelClassName,
  headerActions,
  footerActions,
  floatingCloseClassName = "",
  floatingCloseButtonClassName = "",
  modalRef,
  children,
}: ModalFrameProps) {
  const hasHeaderContent = Boolean(title) || Boolean(headerActions);

  const closeButtonElement = (
    <button
      onClick={onClose}
      className={`flex min-h-[40px] min-w-[40px] items-center justify-center rounded-xl border border-[var(--glass-border-subtle)] text-[var(--color-text-muted)] transition-all duration-300 hover:text-[var(--color-text-primary)] hover:border-[var(--color-text-primary)]/20 active:scale-[0.96] ${
        isDark
          ? "bg-[rgba(255,255,255,0.04)] hover:bg-[rgba(255,255,255,0.08)]"
          : "bg-[rgba(255,255,255,0.88)] hover:bg-[rgba(255,255,255,0.98)]"
      }`}
      aria-label={closeAriaLabel}
    >
      <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2.5} aria-hidden="true">
        <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
      </svg>
    </button>
  );

  return (
    <div
      className={`fixed inset-0 z-50 flex items-stretch justify-center p-0 transition-[opacity,visibility] duration-200 sm:items-center sm:p-4 ${
        isOpen ? "visible opacity-100 animate-fade-in" : "pointer-events-none invisible opacity-0"
      }`}
      style={isOpen ? { backdropFilter: "blur(12px)", WebkitBackdropFilter: "blur(12px)" } : undefined}
      aria-hidden={isOpen ? undefined : true}
    >
      <div
        className={`absolute inset-0 glass-overlay transition-opacity duration-200 ${isOpen ? "opacity-100" : "opacity-0"}`}
        onPointerDown={isOpen ? onClose : undefined}
        aria-hidden="true"
      />

      <div
        className={`relative flex flex-col rounded-none border shadow-2xl transition-[opacity,transform] duration-200 sm:rounded-[28px] glass-modal ${panelClassName} ${
          isOpen ? "opacity-100 animate-scale-in" : "pointer-events-none translate-y-2 scale-[0.985] opacity-0"
        }`}
        ref={modalRef}
        role="dialog"
        aria-modal={isOpen ? "true" : undefined}
        aria-labelledby={titleId}
      >
        {hasHeaderContent ? (
          <div
            className={`flex flex-shrink-0 items-center justify-between gap-4 border-b px-5 py-4 safe-area-inset-top sm:px-6 sm:py-5 border-[var(--glass-border-subtle)] ${
              isDark
                ? "bg-[linear-gradient(180deg,rgba(24,26,31,0.96),var(--color-sidebar-bg))]"
                : "bg-[linear-gradient(180deg,rgba(255,255,255,0.995),var(--color-sidebar-bg))]"
            }`}
          >
            <div id={titleId} className="min-w-0 flex-1 pr-3">
              {title}
            </div>
            <div className="flex flex-shrink-0 items-center gap-2">
              {headerActions}
              {closeButtonElement}
            </div>
          </div>
        ) : (
          <div className={`pointer-events-none absolute right-4 top-4 z-10 sm:right-5 sm:top-5 ${floatingCloseClassName}`}>
            <div className="pointer-events-auto">
              {closeButtonElement}
            </div>
          </div>
        )}

        {children}

        {footerActions && (
          <div className="border-t px-4 py-3 sm:px-6 sm:py-4 safe-area-inset-bottom border-[var(--glass-border-subtle)] glass-header flex-shrink-0">
            {footerActions}
          </div>
        )}
      </div>
    </div>
  );
}
