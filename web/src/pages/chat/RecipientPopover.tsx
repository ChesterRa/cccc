import type { CSSProperties } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";

import { CopyIcon } from "../../components/Icons";
import { classNames } from "../../utils/classNames";
import { RECIPIENT_POPOVER_GAP_PX, type RecipientPopoverTarget } from "./useRecipientPopover";

interface RecipientPopoverProps {
  isDark: boolean;
  target: RecipientPopoverTarget | null;
  style: CSSProperties | null;
  onCancelHide: () => void;
  onScheduleHide: () => void;
  onCopy: (identifier: string) => Promise<void>;
  onHide: () => void;
}

export function RecipientPopover({
  isDark,
  target,
  style,
  onCancelHide,
  onScheduleHide,
  onCopy,
  onHide,
}: RecipientPopoverProps) {
  const { t } = useTranslation("chat");
  if (typeof document === "undefined" || !target || !style) return null;

  return createPortal(
    <div
      className="fixed pointer-events-auto z-[1000]"
      style={style}
      role="dialog"
      aria-label={t("recipientDetails", {
        name: target.label,
        defaultValue: "Recipient details for {{name}}",
      })}
      onMouseEnter={onCancelHide}
      onMouseLeave={onScheduleHide}
      onFocusCapture={onCancelHide}
      onBlurCapture={onScheduleHide}
    >
      <div
        aria-hidden="true"
        className="absolute inset-x-0 top-full"
        style={{ height: RECIPIENT_POPOVER_GAP_PX }}
      />
      <div
        className={classNames(
          "rounded-lg border px-3 py-2 text-xs shadow-xl backdrop-blur-xl",
          isDark
            ? "border-white/12 bg-[rgb(24,25,27)] text-slate-100"
            : "border-black/10 bg-white text-gray-900",
        )}
      >
        <div className="flex min-w-0 items-center gap-2">
          <div className="flex min-w-0 flex-1 items-center gap-2">
            <span
              className={classNames(
                "min-w-0 truncate text-[11px] font-semibold uppercase tracking-wide",
                isDark ? "text-slate-300" : "text-gray-600",
              )}
            >
              {target.kindLabel}
            </span>
            {target.badgeLabel ? (
              <span
                className={classNames(
                  "shrink-0 rounded-full border px-1.5 py-0.5 text-[10px] font-semibold leading-none",
                  isDark
                    ? "border-white/12 bg-white/[0.06] text-slate-300"
                    : "border-black/10 bg-gray-50 text-gray-600",
                )}
              >
                {target.badgeLabel}
              </span>
            ) : null}
          </div>
          <button
            type="button"
            className={classNames(
              "inline-flex h-6 w-6 shrink-0 items-center justify-center rounded-md transition-colors",
              isDark
                ? "text-slate-300 hover:bg-white/[0.1] hover:text-white"
                : "text-gray-500 hover:bg-gray-100 hover:text-gray-950",
            )}
            onClick={() => {
              void onCopy(target.identifier);
              onHide();
            }}
            aria-label={t("copyRecipientIdentifier", { defaultValue: "Copy identifier" })}
            title={t("copyRecipientIdentifier", { defaultValue: "Copy identifier" })}
          >
            <CopyIcon size={13} aria-hidden="true" />
          </button>
        </div>
        {target.idValue ? (
          <div
            className={classNames(
              "mt-2 truncate rounded-md px-2 py-1 font-mono text-[11px]",
              isDark ? "bg-white/[0.08] text-slate-200" : "bg-gray-100 text-gray-800",
            )}
          >
            {target.idValue}
          </div>
        ) : null}
      </div>
    </div>,
    document.body,
  );
}
