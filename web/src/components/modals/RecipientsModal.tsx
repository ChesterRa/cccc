import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";

export type RecipientEntry = readonly [string, boolean];

export interface RecipientsModalProps {
  isOpen: boolean;
  isDark: boolean;
  isSmallScreen: boolean;
  toLabel: string;
  statusKind: "read" | "ack" | "reply";
  entries: RecipientEntry[];
  onClose: () => void;
}

export function RecipientsModal({ isOpen, isDark, isSmallScreen, toLabel, statusKind, entries, onClose }: RecipientsModalProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  if (!isOpen) return null;

  const isAck = statusKind === "ack";
  const isReply = statusKind === "reply";
  const title = isReply ? "Reply Status" : isAck ? "Acknowledgements" : "Recipients";

  return (
    <div
      className={classNames("fixed inset-0 z-50 flex animate-fade-in", isSmallScreen ? "items-end justify-center" : "items-center justify-center p-4")}
      role="dialog"
      aria-modal="true"
      aria-label="Recipient status"
    >
      <div className={classNames("absolute inset-0", isDark ? "bg-black/60" : "bg-black/40")} onPointerDown={onClose} aria-hidden="true" />
      <div
        ref={modalRef}
        className={classNames(
          "relative w-full border shadow-2xl",
          isSmallScreen ? "rounded-t-2xl max-h-[80vh] animate-slide-up safe-area-inset-bottom" : "max-w-md rounded-2xl animate-scale-in",
          isDark ? "bg-slate-900 border-slate-700 text-slate-100" : "bg-white border-gray-200 text-gray-900"
        )}
      >
        <div className={classNames("px-5 py-4 border-b flex items-center justify-between gap-3", isDark ? "border-slate-800" : "border-gray-200")}>
          <div className="min-w-0">
            <div className={classNames("text-sm font-semibold truncate", isDark ? "text-slate-100" : "text-gray-900")}>{title}</div>
            <div className={classNames("text-[11px] truncate", isDark ? "text-slate-500" : "text-gray-500")} title={`to ${toLabel}`}>
              to {toLabel}
            </div>
          </div>
          <button
            type="button"
            className={classNames(
              "touch-target-sm min-w-[36px] min-h-[36px] flex items-center justify-center rounded-lg",
              isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-700 hover:bg-gray-100"
            )}
            onClick={onClose}
            aria-label="Close"
          >
            ×
          </button>
        </div>

        <div className="p-4 sm:p-5 overflow-auto max-h-[70vh]">
          {entries.length > 0 ? (
            <div className={classNames("rounded-xl border divide-y", isDark ? "border-slate-800 divide-slate-800 bg-slate-950/40" : "border-gray-200 divide-gray-200 bg-gray-50")}>
              {entries.map(([id, cleared]) => (
                <div key={id} className="flex items-center justify-between gap-3 px-4 py-3">
                  <div className={classNames("text-sm font-medium truncate", isDark ? "text-slate-200" : "text-gray-800")}>{id}</div>
                  <div
                    className={classNames(
                      "text-sm font-semibold tracking-tight",
                      cleared ? (isDark ? "text-emerald-400" : "text-emerald-600") : isDark ? "text-slate-500" : "text-gray-500"
                    )}
                    aria-label={cleared ? (isReply ? "replied" : isAck ? "acknowledged" : "read") : "pending"}
                  >
                    {isReply ? (cleared ? "↩" : "○") : isAck ? (cleared ? "✓" : "○") : cleared ? "✓✓" : "✓"}
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className={classNames("text-sm py-6 text-center", isDark ? "text-slate-400" : "text-gray-500")}>No recipient tracking for this message.</div>
          )}

          <div className={classNames("text-[11px] mt-3", isDark ? "text-slate-500" : "text-gray-500")}>
            {isReply
              ? "Legend: ○ pending · ↩ replied"
              : isAck
                ? "Legend: ○ pending · ✓ acknowledged"
                : "Legend: ✓ pending · ✓✓ read"}
          </div>
        </div>
      </div>
    </div>
  );
}
