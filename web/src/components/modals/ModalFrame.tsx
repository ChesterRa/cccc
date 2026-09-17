import type { ReactNode, Ref } from "react";

interface ModalFrameProps {
  isOpen?: boolean;
  inline?: boolean;
  isDark: boolean;
  surface?: "glass" | "solid";
  onClose: () => void;
  titleId: string;
  title: ReactNode;
  closeAriaLabel: string;
  panelClassName: string;
  headerActions?: ReactNode;
  headerClassName?: string;
  closeIcon?: ReactNode;
  footerActions?: ReactNode;
  floatingCloseClassName?: string;
  floatingCloseButtonClassName?: string;
  modalRef?: Ref<HTMLDivElement>;
  children: ReactNode;
}

export function ModalFrame({
  isOpen = true,
  inline = false,
  isDark: _isDark,
  surface = "glass",
  onClose,
  titleId,
  title,
  closeAriaLabel,
  panelClassName,
  headerActions,
  headerClassName = "",
  closeIcon,
  footerActions,
  floatingCloseClassName = "",
  floatingCloseButtonClassName = "",
  modalRef,
  children,
}: ModalFrameProps) {
  const hasHeaderContent = Boolean(title) || Boolean(headerActions);

  const closeButtonElement = (
    <button
      type="button"
      onClick={onClose}
      className={`flex min-h-10 min-w-10 max-sm:min-h-11 max-sm:min-w-11 items-center justify-center rounded-lg border border-[var(--color-border-primary)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] ${floatingCloseButtonClassName}`}
      aria-label={closeAriaLabel}
    >
      {closeIcon || (
        <svg
          className="h-4 w-4"
          fill="none"
          viewBox="0 0 24 24"
          stroke="currentColor"
          strokeWidth={2.5}
          aria-hidden="true"
        >
          <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
        </svg>
      )}
    </button>
  );

  return (
    <div
      className={
        inline
          ? `relative flex h-full min-h-0 min-w-0 flex-col ${isOpen ? "" : "hidden"}`
          : `fixed inset-0 z-50 flex items-stretch justify-center p-0 transition-[opacity,visibility] duration-200 sm:items-center sm:p-4 ${
              isOpen
                ? "visible opacity-100 animate-fade-in"
                : "pointer-events-none invisible opacity-0"
            }`
      }
      style={
        !inline && isOpen
          ? { backdropFilter: "blur(12px)", WebkitBackdropFilter: "blur(12px)" }
          : undefined
      }
      aria-hidden={isOpen ? undefined : true}
      inert={!isOpen ? true : undefined}
    >
      <div
        className={`${inline ? "hidden" : ""} absolute inset-0 glass-overlay transition-opacity duration-200 ${isOpen ? "opacity-100" : "opacity-0"}`}
        onPointerDown={isOpen ? onClose : undefined}
        aria-hidden="true"
      />

      <div
        className={
          inline
            ? "relative flex h-full min-h-0 min-w-0 flex-col overflow-hidden"
            : `relative flex flex-col rounded-none border shadow-2xl transition-[opacity,transform] duration-200 ${surface === "solid" ? "sm:rounded-2xl overflow-hidden border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)]" : "sm:rounded-[28px] glass-modal"} ${panelClassName} ${
                isOpen
                  ? "opacity-100 animate-scale-in"
                  : "pointer-events-none translate-y-2 scale-[0.985] opacity-0"
              }`
        }
        ref={modalRef}
        role={inline ? undefined : "dialog"}
        aria-modal={!inline && isOpen ? "true" : undefined}
        aria-labelledby={titleId}
      >
        {hasHeaderContent ? (
          <div
            className={`flex flex-shrink-0 items-center justify-between gap-4 border-b safe-area-inset-top border-[var(--glass-border-subtle)] ${surface === "solid" ? "px-4 py-3 sm:px-5" : "px-5 py-4 sm:px-6 sm:py-5"} ${
              surface === "solid" ? "bg-[var(--color-bg-primary)]" : "bg-[var(--glass-panel-bg)]"
            } ${headerClassName}`}
          >
            <div id={titleId} className="min-w-0 flex-1 pr-3">
              {title}
            </div>
            <div className="flex flex-shrink-0 items-center gap-2">
              {headerActions}
              {closeButtonElement}
            </div>
          </div>
        ) : !inline ? (
          <div
            className={`pointer-events-none absolute right-4 top-4 z-10 sm:right-5 sm:top-5 ${floatingCloseClassName}`}
          >
            <div className="pointer-events-auto">{closeButtonElement}</div>
          </div>
        ) : null}

        {children}

        {footerActions && (
          <div className="flex-shrink-0 border-t border-[var(--glass-border-subtle)] glass-header px-4 pt-3 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))] sm:px-6 sm:pt-4 sm:pb-[calc(1rem+env(safe-area-inset-bottom,0px))]">
            {footerActions}
          </div>
        )}
      </div>
    </div>
  );
}
