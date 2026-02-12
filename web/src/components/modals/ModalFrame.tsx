import type { ReactNode, Ref } from "react";

interface ModalFrameProps {
  isDark: boolean;
  onClose: () => void;
  titleId: string;
  title: ReactNode;
  closeAriaLabel: string;
  panelClassName: string;
  modalRef?: Ref<HTMLDivElement>;
  children: ReactNode;
}

export function ModalFrame({
  isDark,
  onClose,
  titleId,
  title,
  closeAriaLabel,
  panelClassName,
  modalRef,
  children,
}: ModalFrameProps) {
  return (
    <div className="fixed inset-0 z-50 flex items-stretch sm:items-center justify-center p-0 sm:p-4 animate-fade-in">
      <div
        className={isDark ? "absolute inset-0 bg-black/60" : "absolute inset-0 bg-black/40"}
        onPointerDown={onClose}
        aria-hidden="true"
      />

      <div
        className={`relative flex flex-col border shadow-2xl animate-scale-in rounded-none sm:rounded-xl ${
          isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
        } ${panelClassName}`}
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
      >
        <div
          className={`flex flex-shrink-0 items-center justify-between px-5 py-4 border-b safe-area-inset-top ${
            isDark ? "border-slate-800" : "border-gray-200"
          }`}
        >
          <h2 id={titleId} className={`text-lg font-semibold ${isDark ? "text-slate-100" : "text-gray-900"}`}>
            {title}
          </h2>
          <button
            onClick={onClose}
            className={`text-xl leading-none min-w-[44px] min-h-[44px] flex items-center justify-center rounded-lg transition-colors ${
              isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
            }`}
            aria-label={closeAriaLabel}
          >
            ×
          </button>
        </div>

        {children}
      </div>
    </div>
  );
}
