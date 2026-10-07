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
      className="shrink-0 border-b border-[var(--glass-border-subtle)] px-2 py-1.5 max-sm:px-3 max-sm:py-2.5"
    >
      <div className="grid grid-cols-4 gap-1 rounded-lg bg-[var(--glass-tab-bg)] p-1 max-sm:gap-1.5 max-sm:rounded-xl">
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
                "flex min-h-8 min-w-0 items-center justify-center gap-1 rounded-md border text-xs tabular-nums focus-visible:outline focus-visible:outline-2 disabled:opacity-40 max-sm:min-h-10 max-sm:rounded-lg max-sm:text-[13px]",
                slot.slot_id === activeSlotId
                  ? "border-transparent bg-[var(--color-text-primary)] font-semibold text-[var(--color-bg-primary)]"
                  : slot.card
                    ? "border-transparent text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-primary)]"
                    : "border-dashed border-[var(--color-border-primary)] text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]",
              )}
            >
              {!slot.card && <Plus className="h-3 w-3" aria-hidden="true" />}
              {index + 1}
            </button>
          );
        })}
      </div>
    </div>
  );
}
