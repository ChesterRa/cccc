import { useLayoutEffect, useMemo, useRef, type MouseEvent } from "react";
import { useTranslation } from "react-i18next";
import { PanelRightClose, PanelRightOpen, Plus, X } from "lucide-react";
import type { GroupPresentation } from "../../types";
import { classNames } from "../../utils/classNames";
import { ensurePresentation } from "../../utils/presentation";
import { SidePanelButton, SidePanelHeader } from "../layout/SidePanelHeader";
import { PresentationSlotPreview } from "./PresentationSlotPreview";

type PresentationRailProps = {
  groupId: string;
  presentation: GroupPresentation | null;
  isDark: boolean;
  readOnly?: boolean;
  compact?: boolean;
  onToggleCompact?: () => void;
  onClose?: () => void;
  attentionSlots?: Record<string, boolean>;
  onOpenSlot: (slotId: string) => void;
  onPinSlot?: (slotId: string) => void;
};

export function PresentationRail({
  groupId,
  presentation,
  isDark,
  readOnly,
  compact = false,
  onToggleCompact,
  onClose,
  attentionSlots,
  onOpenSlot,
  onPinSlot,
}: PresentationRailProps) {
  const { t, i18n } = useTranslation("chat");
  const normalized = useMemo(() => ensurePresentation(presentation), [presentation]);
  const filled = normalized.slots.filter((slot) => slot.card).length;
  const date = new Date(normalized.updated_at || "");
  const updated =
    filled && Number.isFinite(date.getTime())
      ? date.toLocaleString(i18n.language, {
          month: "short",
          day: "numeric",
          hour: "2-digit",
          minute: "2-digit",
        })
      : "";
  const toggleRef = useRef<HTMLButtonElement>(null);
  const restoreToggleFocus = useRef(false);
  useLayoutEffect(() => {
    if (restoreToggleFocus.current) toggleRef.current?.focus();
    restoreToggleFocus.current = false;
  }, [compact]);
  const toggleCompact = (event: MouseEvent<HTMLButtonElement>) => {
    restoreToggleFocus.current = document.activeElement === event.currentTarget;
    onToggleCompact?.();
  };
  const closeLabel = t("presentationCloseDockAction");
  return (
    <section
      className="@container flex h-full min-h-0 w-full flex-col"
      aria-label={t("presentationSectionLabel")}
      data-presentation-density={compact ? "compact" : "expanded"}
    >
      {compact ? (
        <div className="flex shrink-0 justify-center border-b border-[var(--glass-border-subtle)] py-1.5">
          <SidePanelButton
            title={t("presentationExpandSlots")}
            ref={toggleRef}
            onClick={toggleCompact}
          >
            <PanelRightOpen />
          </SidePanelButton>
        </div>
      ) : (
        <SidePanelHeader
          title={t("presentationTitle")}
          subtitle={`${filled}/${normalized.slots.length}${updated ? ` · ${t("presentationUpdatedAt", { value: updated })}` : ""}`}
          onClose={onClose}
          closeLabel={closeLabel}
        >
          {onToggleCompact && (
            <SidePanelButton
              title={t("presentationCompactSlots")}
              ref={toggleRef}
              onClick={toggleCompact}
            >
              <PanelRightClose />
            </SidePanelButton>
          )}
        </SidePanelHeader>
      )}
      <div
        className={classNames(
          "min-h-0 flex-1 overflow-y-auto scrollbar-subtle",
          compact ? "p-2" : "p-3",
        )}
      >
        <div className={"flex flex-col gap-2"}>
          {normalized.slots.map((slot, index) => {
            const card = slot.card;
            const attention = !!attentionSlots?.[slot.slot_id];
            const label = card
              ? t("presentationOpenSlot", { index: index + 1, title: card.title })
              : readOnly || !onPinSlot
                ? `${index + 1} · ${t("presentationSlotEmptyTitle")}`
                : t("presentationPinSlotTitle", { index: index + 1 });
            return (
              <button
                key={slot.slot_id}
                type="button"
                title={card?.title || t("presentationSlotEmptyTitle")}
                aria-label={attention ? `${label} · ${t("presentationUpdatedNotice")}` : label}
                disabled={!card && (readOnly || !onPinSlot)}
                onClick={() => (card ? onOpenSlot(slot.slot_id) : onPinSlot?.(slot.slot_id))}
                className={classNames(
                  "relative min-w-0 overflow-hidden rounded-lg border border-[var(--glass-panel-border)] text-left transition-colors hover:bg-[var(--glass-tab-bg)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 disabled:cursor-default disabled:opacity-60",
                  compact
                    ? "flex h-12 w-full items-center justify-center"
                    : "flex items-center gap-3 p-2.5",
                  !card && "border-dashed",
                  slot.slot_id === normalized.highlight_slot_id && "bg-[var(--glass-tab-bg)]",
                  attention && (isDark ? "ring-2 ring-cyan-300/60" : "ring-2 ring-cyan-600/50"),
                )}
              >
                {card ? (
                  <>
                    <div
                      className={classNames(
                        "pointer-events-none flex items-center justify-center overflow-hidden",
                        compact
                          ? "h-7 w-7"
                          : "h-12 w-16 shrink-0 rounded-md bg-[var(--color-bg-secondary)]",
                      )}
                    >
                      <PresentationSlotPreview
                        key={`${slot.slot_id}:${card.published_at}:${card.content.url || ""}`}
                        groupId={groupId}
                        slot={slot}
                      />
                    </div>
                    {!compact && (
                      <div className="flex min-w-0 flex-1 items-start gap-2">
                        <span className="text-xs tabular-nums text-[var(--color-text-tertiary)]">
                          {index + 1}
                        </span>
                        <span className="min-w-0 flex-1">
                          <span className="block truncate text-sm font-medium">{card.title}</span>
                          {(card.summary ||
                            card.content.url ||
                            card.content.workspace_rel_path) && (
                            <span className="mt-1 block truncate text-xs text-[var(--color-text-secondary)]">
                              {card.summary || card.content.url || card.content.workspace_rel_path}
                            </span>
                          )}
                        </span>
                      </div>
                    )}
                    {compact && (
                      <span className="absolute bottom-0.5 left-1 text-[9px] text-[var(--color-text-tertiary)]">
                        {index + 1}
                      </span>
                    )}
                  </>
                ) : (
                  <div
                    className={classNames(
                      "flex items-center justify-center gap-2 text-[var(--color-text-tertiary)]",
                      !compact && "min-h-6 text-xs",
                    )}
                  >
                    {readOnly || !onPinSlot ? (
                      <span>{index + 1}</span>
                    ) : (
                      <Plus className="h-4 w-4" />
                    )}
                    {!compact && (
                      <span>
                        {readOnly || !onPinSlot
                          ? t("presentationSlotEmptyTitle")
                          : t("presentationPinSlotTitle", { index: index + 1 })}
                      </span>
                    )}
                  </div>
                )}
                {attention && (
                  <span
                    className="absolute right-1 top-1 h-1.5 w-1.5 rounded-full bg-cyan-500"
                    aria-hidden="true"
                  />
                )}
              </button>
            );
          })}
        </div>
        {!compact && filled === 0 && (
          <p className="mt-3 text-xs leading-5 text-[var(--color-text-tertiary)]">
            {t(readOnly ? "presentationEmptyReadOnlyHint" : "presentationEmptyActionHint")}
          </p>
        )}
      </div>
      {compact && onClose && (
        <div className="flex shrink-0 justify-center border-t border-[var(--glass-border-subtle)] py-1">
          <SidePanelButton title={closeLabel} onClick={onClose}>
            <X />
          </SidePanelButton>
        </div>
      )}
    </section>
  );
}
