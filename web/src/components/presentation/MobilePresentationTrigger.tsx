import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { GroupPresentation } from "../../types";
import { classNames } from "../../utils/classNames";
import { BookmarkIcon } from "../Icons";
import { resolveMobilePresentationHighlight } from "./mobilePresentationModel";

export function MobilePresentationTrigger({
  presentation,
  attentionSlots,
  isDark,
  onOpen,
}: {
  presentation: GroupPresentation | null;
  attentionSlots: Record<string, boolean>;
  isDark: boolean;
  onOpen: () => void;
}) {
  const { t } = useTranslation("chat");
  const highlightedSlot = useMemo(
    () => resolveMobilePresentationHighlight(presentation),
    [presentation],
  );
  const hasAttention = Object.keys(attentionSlots).length > 0;
  const title = String(highlightedSlot?.card?.title || "").trim();
  const accessibleLabel = highlightedSlot
    ? t("presentationMobileOpenHighlighted", {
        index: highlightedSlot.index,
        title,
        defaultValue: `Open Presentation, slot ${highlightedSlot.index}: ${title}`,
      })
    : t("presentationOpenDockAction", { defaultValue: "Open presentation" });

  return (
    <button
      type="button"
      onClick={onOpen}
      className={classNames(
        "pointer-events-auto relative flex h-11 flex-shrink-0 items-center gap-1.5 rounded-full border px-3 backdrop-blur-xl transition-all duration-200",
        isDark
          ? "border-white/10 bg-slate-950/74 text-slate-100 shadow-lg shadow-black/20"
          : "border-black/10 bg-white/88 text-gray-900 shadow-sm",
        hasAttention &&
          (isDark
            ? "presentation-slot-attention presentation-slot-attention-dark"
            : "presentation-slot-attention presentation-slot-attention-light"),
      )}
      aria-label={accessibleLabel}
      title={accessibleLabel}
      data-mobile-presentation-trigger="true"
    >
      <BookmarkIcon size={17} className="shrink-0" aria-hidden="true" />
      <span className="sr-only">{t("presentationTitle", { defaultValue: "Presentation" })}</span>
      {highlightedSlot ? (
        <span
          className={classNames(
            "inline-flex h-6 min-w-6 shrink-0 items-center justify-center rounded-full px-1.5 text-[10px] font-bold",
            isDark ? "bg-white/10 text-white" : "bg-black/6 text-gray-800",
          )}
          aria-label={t("presentationHighlightedSlotLabel", {
            index: highlightedSlot.index,
            defaultValue: `Highlighted slot ${highlightedSlot.index}`,
          })}
        >
          {highlightedSlot.index}
        </span>
      ) : null}
      {hasAttention ? (
        <span
          className={classNames(
            "absolute right-1 top-1 h-2 w-2 rounded-full",
            isDark ? "bg-cyan-200" : "bg-cyan-500",
          )}
          aria-hidden="true"
        />
      ) : null}
    </button>
  );
}
