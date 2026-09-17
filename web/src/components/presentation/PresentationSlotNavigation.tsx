import type { RefObject } from "react";
import { useTranslation } from "react-i18next";
import { Plus } from "lucide-react";
import type { GroupPresentation } from "../../types";
import { ensurePresentation } from "../../utils/presentation";
import { classNames } from "../../utils/classNames";

/** Keep the four fixed slots reachable while reading one of them. */
export function PresentationSlotNavigation({
  presentation,
  activeSlotId,
  selectedButtonRef,
  readOnly,
  onSelectSlot,
  onPinSlot,
}: {
  presentation: GroupPresentation | null;
  activeSlotId: string;
  selectedButtonRef?: RefObject<HTMLButtonElement | null>;
  readOnly?: boolean;
  onSelectSlot: (slotId: string) => void;
  onPinSlot?: (slotId: string) => void;
}) {
  const { t } = useTranslation("chat");
  return (
    <div
      role="group"
      aria-label={t("presentationSectionLabel")}
      data-presentation-slot-navigation
      className="flex shrink-0 gap-1 border-b border-[var(--glass-border-subtle)] px-2 py-1.5"
    >
      {ensurePresentation(presentation).slots.map((slot, index) => {
        const label = slot.card
          ? t("presentationOpenSlot", { index: index + 1, title: slot.card.title })
          : readOnly || !onPinSlot
            ? `${index + 1} · ${t("presentationSlotEmptyTitle")}`
            : t("presentationPinSlotTitle", { index: index + 1 });
        return (
          <button
            key={slot.slot_id}
            ref={slot.slot_id === activeSlotId ? selectedButtonRef : undefined}
            type="button"
            title={label}
            aria-label={label}
            aria-pressed={slot.slot_id === activeSlotId}
            disabled={!slot.card && (readOnly || !onPinSlot)}
            onClick={() => (slot.card ? onSelectSlot(slot.slot_id) : onPinSlot?.(slot.slot_id))}
            className={classNames(
              "flex min-h-9 min-w-0 flex-1 items-center justify-center gap-1 rounded-md border text-xs tabular-nums text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline focus-visible:outline-2 disabled:opacity-40",
              slot.slot_id === activeSlotId
                ? "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg-active)] font-semibold text-[var(--color-text-primary)]"
                : "border-transparent",
            )}
          >
            {!slot.card && <Plus className="h-3 w-3" aria-hidden="true" />}
            {index + 1}
          </button>
        );
      })}
    </div>
  );
}
