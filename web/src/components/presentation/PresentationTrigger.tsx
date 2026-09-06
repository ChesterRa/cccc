import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { GroupPresentation } from "../../types";
import { classNames } from "../../utils/classNames";
import { BookmarkIcon } from "../Icons";
import { ensurePresentation } from "../../utils/presentation";

export function PresentationTrigger({
  presentation,
  attentionSlots,
  isDark,
  onOpen,
  isOpen = false,
  mobile = false,
}: {
  presentation: GroupPresentation | null;
  attentionSlots: Record<string, boolean>;
  isDark: boolean;
  onOpen: () => void;
  isOpen?: boolean;
  mobile?: boolean;
}) {
  const { t } = useTranslation("chat");
  const highlightedSlot = useMemo(() => {
    const normalized = ensurePresentation(presentation);
    return (
      normalized.slots.find((slot) => slot.slot_id === normalized.highlight_slot_id && slot.card) ||
      normalized.slots.find((slot) => slot.card) ||
      null
    );
  }, [presentation]);
  const hasAttention = Object.keys(attentionSlots).length > 0;
  const title = String(highlightedSlot?.card?.title || "").trim();
  const accessibleLabel = isOpen
    ? t("presentationCloseDockAction")
    : highlightedSlot
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
        "relative flex h-8 w-8 shrink-0 items-center justify-center rounded-md text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)]",
        hasAttention &&
          (isDark
            ? "presentation-slot-attention presentation-slot-attention-dark"
            : "presentation-slot-attention presentation-slot-attention-light"),
      )}
      aria-label={accessibleLabel}
      title={accessibleLabel}
      data-mobile-presentation-trigger={mobile ? "true" : undefined}
      data-group-presentation-trigger
      aria-expanded={isOpen}
    >
      <BookmarkIcon size={17} className="shrink-0" aria-hidden="true" />
      <span className="sr-only">{t("presentationTitle", { defaultValue: "Presentation" })}</span>
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
