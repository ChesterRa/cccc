import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import type { GroupPresentation, PresentationSlot } from "../../types";
import { ChevronLeftIcon } from "../Icons";
import { classNames } from "../../utils/classNames";
import { ensurePresentation } from "../../utils/presentation";

type PresentationRailProps = {
  presentation: GroupPresentation | null;
  isDark: boolean;
  readOnly?: boolean;
  onClose?: () => void;
  attentionSlots?: Record<string, boolean>;
  onOpenSlot: (slotId: string) => void;
  onPinSlot?: (slotId: string) => void;
};

function formatUpdatedAt(value: string | undefined, locale: string): string {
  const raw = String(value || "").trim();
  if (!raw) return "";
  const parsed = new Date(raw);
  if (Number.isNaN(parsed.getTime())) return "";
  return parsed.toLocaleString(locale, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function getCardTypeLabel(
  type: string,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  switch (String(type || "").trim()) {
    case "markdown":
      return t("presentationTypeMarkdown", { defaultValue: "Markdown" });
    case "table":
      return t("presentationTypeTable", { defaultValue: "Table" });
    case "image":
      return t("presentationTypeImage", { defaultValue: "Image" });
    case "pdf":
      return t("presentationTypePdf", { defaultValue: "PDF" });
    case "web_preview":
      return t("presentationTypeWebPreview", { defaultValue: "Web" });
    default:
      return t("presentationTypeFile", { defaultValue: "File" });
  }
}

function getFilledSlots(presentation: GroupPresentation | null): PresentationSlot[] {
  return Array.isArray(presentation?.slots) ? presentation.slots.filter((slot) => !!slot.card) : [];
}

function getPreviewText(
  slot: PresentationSlot,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  const card = slot.card;
  if (!card) return "";
  const summary = String(card.summary || "").trim();
  if (summary) return summary;
  const sourceLabel = String(card.source_label || "").trim();
  if (sourceLabel) return sourceLabel;
  if (card.card_type === "table") {
    const rowCount = card.content.table?.rows?.length || 0;
    return t("presentationRowsSummary", { count: rowCount, defaultValue: `${rowCount} rows` });
  }
  return getCardTypeLabel(card.card_type, t);
}

export function PresentationRail({
  presentation,
  isDark,
  readOnly,
  onClose,
  attentionSlots,
  onOpenSlot,
  onPinSlot,
}: PresentationRailProps) {
  const { t, i18n } = useTranslation("chat");
  const normalizedPresentation = useMemo(() => ensurePresentation(presentation), [presentation]);
  const filledSlots = useMemo(
    () => getFilledSlots(normalizedPresentation),
    [normalizedPresentation],
  );
  const hasCards = filledSlots.length > 0;
  const highlightSlotId = String(normalizedPresentation.highlight_slot_id || "").trim();

  const updatedAt = formatUpdatedAt(normalizedPresentation.updated_at, i18n.language);
  return (
    <section
      className={classNames(
        "flex h-full min-h-0 flex-col",
        isDark ? "bg-slate-950/20" : "bg-white/40",
      )}
      aria-label={t("presentationSectionLabel", { defaultValue: "Presentation" })}
    >
      <div
        className={classNames(
          "flex items-center justify-between gap-3 px-4 py-2 border-b",
          isDark ? "border-white/5" : "border-black/5",
        )}
      >
        <div className="min-w-0">
          <h2
            className={classNames(
              "text-sm font-semibold",
              isDark ? "text-slate-100" : "text-gray-900",
            )}
          >
            {t("presentationTitle", { defaultValue: "Presentation" })}
          </h2>
          <p className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-600")}>
            {hasCards && updatedAt
              ? t("presentationUpdatedAt", {
                  value: updatedAt,
                  defaultValue: `Updated ${updatedAt}`,
                })
              : t("presentationEmptyHelp", {
                  defaultValue: "Tap an empty slot to pin a URL or a local file.",
                })}
          </p>
        </div>
        <div className="flex items-center gap-3">
          <div
            className={classNames(
              "text-xs font-medium",
              isDark ? "text-slate-400" : "text-gray-500",
            )}
          >
            {filledSlots.length}/{normalizedPresentation.slots.length}
          </div>
          {onClose ? (
            <button
              type="button"
              onClick={onClose}
              className={classNames(
                "flex h-10 w-10 items-center justify-center rounded-full border backdrop-blur-xl transition-all duration-200",
                isDark
                  ? "border-white/10 bg-slate-950/62 text-slate-100 hover:border-white/16 hover:bg-slate-900/82"
                  : "border-black/10 bg-white/78 text-gray-900 hover:border-black/14 hover:bg-white/92",
              )}
              title={t("presentationCloseDockAction", { defaultValue: "Hide presentation" })}
              aria-label={t("presentationCloseDockAction", { defaultValue: "Hide presentation" })}
              data-mobile-presentation-close="true"
            >
              <ChevronLeftIcon size={20} aria-hidden="true" />
            </button>
          ) : null}
        </div>
      </div>

      <div className="flex-1 min-h-0 overflow-auto p-4">
        <div className="grid grid-cols-1 gap-3 min-[420px]:grid-cols-2">
          {normalizedPresentation.slots.map((slot) => {
            const card = slot.card;
            const isHighlighted = slot.slot_id === highlightSlotId;
            const hasSlotAttention = !!attentionSlots?.[slot.slot_id];
            return (
              <button
                key={slot.slot_id}
                type="button"
                onClick={() => {
                  if (card) {
                    onOpenSlot(slot.slot_id);
                    return;
                  }
                  if (!readOnly) {
                    onPinSlot?.(slot.slot_id);
                  }
                }}
                className={classNames(
                  "relative rounded-3xl border p-4 text-left transition-all",
                  "min-h-[164px] shadow-sm hover:-translate-y-0.5",
                  isDark
                    ? "border-white/10 bg-slate-900/70 hover:border-white/18"
                    : "border-black/10 bg-white/85 hover:border-black/16",
                  !card &&
                    readOnly &&
                    (isDark ? "cursor-default opacity-80" : "cursor-default opacity-90"),
                  isHighlighted &&
                    (isDark
                      ? "ring-2 ring-[rgb(143,163,187)]/38"
                      : "ring-2 ring-[rgb(62,80,103)]/18"),
                  hasSlotAttention &&
                    (isDark
                      ? "ring-2 ring-cyan-300/70 presentation-slot-attention presentation-slot-attention-dark"
                      : "ring-2 ring-cyan-500/60 presentation-slot-attention presentation-slot-attention-light"),
                )}
                aria-label={t("presentationOpenSlot", {
                  index: slot.index,
                  title: card?.title || t("presentationSlotEmpty", { defaultValue: "Empty" }),
                  defaultValue: `Open presentation slot ${slot.index}: ${card?.title || "Empty"}`,
                })}
              >
                <div className="flex items-start justify-between gap-3">
                  <span
                    className={classNames(
                      "inline-flex h-8 min-w-[2rem] items-center justify-center rounded-full px-2 text-xs font-semibold",
                      isDark ? "bg-slate-800 text-slate-200" : "bg-gray-100 text-gray-700",
                    )}
                  >
                    {slot.index}
                  </span>
                  <span
                    className={classNames(
                      "rounded-full px-2 py-1 text-[11px] font-medium",
                      card
                        ? isDark
                          ? "bg-white/[0.08] text-white"
                          : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
                        : isDark
                          ? "bg-slate-800 text-slate-300"
                          : "bg-gray-100 text-gray-600",
                    )}
                  >
                    {card
                      ? getCardTypeLabel(card.card_type, t)
                      : t("presentationPinAction", { defaultValue: "Pin" })}
                  </span>
                </div>
                <div
                  className={classNames(
                    "mt-4 text-sm font-semibold leading-5",
                    isDark ? "text-slate-100" : "text-gray-900",
                  )}
                >
                  {card
                    ? card.title
                    : t("presentationSlotEmptyTitle", { defaultValue: "Empty slot" })}
                </div>
                <div
                  className={classNames(
                    "mt-2 text-xs leading-5",
                    isDark ? "text-slate-400" : "text-gray-600",
                  )}
                >
                  {card
                    ? getPreviewText(slot, t)
                    : readOnly
                      ? t("presentationEmptyReadOnlyHint", {
                          defaultValue:
                            "Waiting for an agent or an authorized user to publish here.",
                        })
                      : t("presentationEmptyActionHint", {
                          defaultValue: "Tap to pin a URL or upload a local file.",
                        })}
                </div>
              </button>
            );
          })}
        </div>
      </div>
    </section>
  );
}
