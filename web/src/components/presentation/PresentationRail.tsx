import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import type { GroupPresentation, PresentationSlot } from "../../types";
import { BookmarkIcon } from "../Icons";
import { classNames } from "../../utils/classNames";
import { ensurePresentation } from "../../utils/presentation";

type PresentationRailProps = {
  mode: "dock" | "panel" | "split";
  presentation: GroupPresentation | null;
  isDark: boolean;
  readOnly?: boolean;
  isOpen?: boolean;
  onOpenChange?: (open: boolean) => void;
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

function getSlotTone(
  cardType: string,
  isDark: boolean,
): { buttonClassName: string; indicatorClassName: string } {
  switch (String(cardType || "").trim()) {
    case "markdown":
      return isDark
        ? {
            buttonClassName:
              "border-emerald-400/28 bg-emerald-400/[0.08] text-emerald-100 hover:border-emerald-300/45 hover:bg-emerald-400/[0.12]",
            indicatorClassName: "bg-emerald-300 ring-emerald-100/20",
          }
        : {
            buttonClassName:
              "border-emerald-500/25 bg-emerald-50/90 text-emerald-900 hover:border-emerald-500/40 hover:bg-emerald-50",
            indicatorClassName: "bg-emerald-500 ring-emerald-100",
          };
    case "table":
      return isDark
        ? {
            buttonClassName:
              "border-amber-400/28 bg-amber-400/[0.08] text-amber-100 hover:border-amber-300/45 hover:bg-amber-400/[0.12]",
            indicatorClassName: "bg-amber-300 ring-amber-100/20",
          }
        : {
            buttonClassName:
              "border-amber-500/25 bg-amber-50/90 text-amber-900 hover:border-amber-500/40 hover:bg-amber-50",
            indicatorClassName: "bg-amber-500 ring-amber-100",
          };
    case "image":
      return isDark
        ? {
            buttonClassName:
              "border-[rgb(143,163,187)]/24 bg-[rgb(143,163,187)]/[0.08] text-slate-100 hover:border-[rgb(143,163,187)]/38 hover:bg-[rgb(143,163,187)]/[0.12]",
            indicatorClassName: "bg-[rgb(143,163,187)] ring-white/10",
          }
        : {
            buttonClassName:
              "border-[rgb(35,36,37)]/12 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:border-[rgb(35,36,37)]/22 hover:bg-[rgb(240,240,240)]",
            indicatorClassName: "bg-[rgb(62,80,103)] ring-black/5",
          };
    case "pdf":
      return isDark
        ? {
            buttonClassName:
              "border-rose-400/28 bg-rose-400/[0.08] text-rose-100 hover:border-rose-300/45 hover:bg-rose-400/[0.12]",
            indicatorClassName: "bg-rose-300 ring-rose-100/20",
          }
        : {
            buttonClassName:
              "border-rose-500/25 bg-rose-50/90 text-rose-900 hover:border-rose-500/40 hover:bg-rose-50",
            indicatorClassName: "bg-rose-500 ring-rose-100",
          };
    case "web_preview":
      return isDark
        ? {
            buttonClassName:
              "border-[rgb(143,163,187)]/24 bg-[rgb(143,163,187)]/[0.08] text-slate-100 hover:border-[rgb(143,163,187)]/38 hover:bg-[rgb(143,163,187)]/[0.12]",
            indicatorClassName: "bg-[rgb(143,163,187)] ring-white/10",
          }
        : {
            buttonClassName:
              "border-[rgb(35,36,37)]/12 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:border-[rgb(35,36,37)]/22 hover:bg-[rgb(240,240,240)]",
            indicatorClassName: "bg-[rgb(62,80,103)] ring-black/5",
          };
    default:
      return isDark
        ? {
            buttonClassName:
              "border-slate-300/18 bg-slate-200/[0.07] text-slate-100 hover:border-slate-200/28 hover:bg-slate-200/[0.1]",
            indicatorClassName: "bg-slate-200 ring-slate-50/10",
          }
        : {
            buttonClassName:
              "border-slate-400/20 bg-slate-50/90 text-slate-800 hover:border-slate-400/34 hover:bg-slate-50",
            indicatorClassName: "bg-slate-500 ring-slate-100",
          };
  }
}

function getDockPreviewPlacement(index: number, total: number): string {
  if (index <= 1) return "top-0";
  if (index >= total) return "bottom-0";
  return "top-1/2 -translate-y-1/2";
}

export function PresentationRail({
  mode,
  presentation,
  isDark,
  readOnly,
  isOpen = false,
  onOpenChange,
  attentionSlots,
  onOpenSlot,
  onPinSlot,
}: PresentationRailProps) {
  const { t, i18n } = useTranslation("chat");
  const [hoveredSlotId, setHoveredSlotId] = useState("");
  const normalizedPresentation = useMemo(() => ensurePresentation(presentation), [presentation]);
  const filledSlots = useMemo(
    () => getFilledSlots(normalizedPresentation),
    [normalizedPresentation],
  );
  const hasCards = filledSlots.length > 0;
  const highlightSlotId = String(normalizedPresentation.highlight_slot_id || "").trim();
  const hasAttention = Object.keys(attentionSlots || {}).length > 0;

  useEffect(() => {
    if (mode !== "dock" || !isOpen) return undefined;
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onOpenChange?.(false);
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [isOpen, mode, onOpenChange]);

  if (mode === "panel") {
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
            {onOpenChange ? (
              <button
                type="button"
                onClick={() => onOpenChange(false)}
                className={classNames(
                  "flex h-10 w-10 items-center justify-center rounded-full border backdrop-blur-xl transition-all duration-200",
                  isDark
                    ? "border-white/10 bg-slate-950/62 text-slate-100 hover:border-white/16 hover:bg-slate-900/82"
                    : "border-black/10 bg-white/78 text-gray-900 hover:border-black/14 hover:bg-white/92",
                )}
                title={t("presentationCloseDockAction", { defaultValue: "Hide presentation" })}
                aria-label={t("presentationCloseDockAction", { defaultValue: "Hide presentation" })}
                aria-expanded={isOpen}
              >
                <BookmarkIcon size={18} />
              </button>
            ) : null}
          </div>
        </div>

        <div className="flex-1 min-h-0 overflow-auto p-4">
          <div className="grid grid-cols-2 gap-3">
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
  if (mode === "split") {
    return (
      <aside
        className={classNames(
          "flex h-full w-[52px] flex-shrink-0 flex-col items-center border-r px-1 py-2",
          isDark ? "border-white/8 bg-slate-950/24" : "border-black/8 bg-white/56",
        )}
        aria-label={t("presentationDockAriaLabel", { defaultValue: "Presentation slots" })}
      >
        <div
          className={classNames(
            "inline-flex min-h-[20px] items-center justify-center rounded-full px-1.5 text-[10px] font-medium",
            isDark ? "bg-white/[0.06] text-slate-300" : "bg-black/[0.05] text-gray-600",
          )}
        >
          {filledSlots.length}/{normalizedPresentation.slots.length}
        </div>

        <div className="mt-2 flex flex-1 flex-col items-center gap-1.5">
          {normalizedPresentation.slots.map((slot) => {
            const card = slot.card;
            const isHighlighted = slot.slot_id === highlightSlotId;
            const hasSlotAttention = !!attentionSlots?.[slot.slot_id];
            const tone = card ? getSlotTone(card.card_type, isDark) : null;
            const title = card
              ? card.title
              : t("presentationSlotEmptyTitle", { defaultValue: "Empty slot" });
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
                  "group relative flex h-10 w-10 items-center justify-center rounded-[14px] border text-center transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35",
                  card || !readOnly ? "cursor-pointer" : "cursor-default",
                  card
                    ? tone?.buttonClassName
                    : isDark
                      ? "border-dashed border-white/10 bg-white/[0.03] text-slate-100 hover:border-white/16 hover:bg-white/[0.05]"
                      : "border-dashed border-black/10 bg-white/78 text-gray-900 hover:border-black/16 hover:bg-white",
                  !card &&
                    !readOnly &&
                    (isDark ? "hover:border-white/18" : "hover:border-black/14"),
                  isHighlighted &&
                    (isDark
                      ? "ring-1 ring-[rgb(143,163,187)]/32"
                      : "ring-1 ring-[rgb(62,80,103)]/16"),
                  hasSlotAttention &&
                    (isDark
                      ? "ring-2 ring-cyan-300/70 presentation-slot-attention presentation-slot-attention-dark"
                      : "ring-2 ring-cyan-500/60 presentation-slot-attention presentation-slot-attention-light"),
                )}
                aria-label={
                  card
                    ? t("presentationOpenSlot", {
                        index: slot.index,
                        title: card.title,
                        defaultValue: `Open presentation slot ${slot.index}: ${card.title}`,
                      })
                    : t("presentationEmptySlot", {
                        index: slot.index,
                        defaultValue: `Presentation slot ${slot.index} is empty`,
                      })
                }
                title={title}
              >
                {card ? (
                  <span
                    className={classNames(
                      "pointer-events-none absolute right-1 top-1 h-2 w-2 rounded-full ring-1",
                      tone?.indicatorClassName,
                    )}
                  />
                ) : null}
                <span
                  className={classNames(
                    "text-[13px] font-semibold tracking-[0.01em]",
                    card ? "text-current" : isDark ? "text-slate-200" : "text-gray-800",
                  )}
                >
                  {slot.index}
                </span>
              </button>
            );
          })}
        </div>
      </aside>
    );
  }

  return (
    <div
      className="pointer-events-none absolute right-4 top-4 z-30 flex flex-col items-end gap-2"
      aria-label={t("presentationTitle", { defaultValue: "Presentation" })}
    >
      <button
        type="button"
        onClick={() => onOpenChange?.(!isOpen)}
        className={classNames(
          "pointer-events-auto group relative flex h-12 w-12 items-center justify-center rounded-full border backdrop-blur-xl transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35",
          isDark
            ? "border-white/10 bg-slate-950/62 text-slate-100 shadow-[0_20px_48px_-28px_rgba(2,6,23,0.72)]"
            : "border-black/10 bg-white/78 text-gray-900 shadow-[0_20px_48px_-28px_rgba(15,23,42,0.18)]",
          hasCards &&
            !hasAttention &&
            (isDark
              ? "border-white/12 bg-white/[0.07] text-white"
              : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"),
          isOpen
            ? isDark
              ? "border-white/18 bg-slate-950/84"
              : "border-black/14 bg-white/92"
            : isDark
              ? "hover:border-white/16 hover:bg-slate-900/82"
              : "hover:border-black/14 hover:bg-white/92",
          hasAttention &&
            (isDark
              ? "presentation-slot-attention presentation-slot-attention-dark"
              : "presentation-slot-attention presentation-slot-attention-light"),
        )}
        title={
          isOpen
            ? t("presentationCloseDockAction", { defaultValue: "Hide presentation" })
            : t("presentationOpenDockAction", { defaultValue: "Open presentation" })
        }
        aria-label={
          isOpen
            ? t("presentationCloseDockAction", { defaultValue: "Hide presentation" })
            : t("presentationOpenDockAction", { defaultValue: "Open presentation" })
        }
        aria-expanded={isOpen}
      >
        <span className="relative flex items-center justify-center">
          <BookmarkIcon
            size={18}
            className={classNames(
              "drop-shadow-[0_1px_1px_rgba(15,23,42,0.18)] transition-transform duration-200",
              isOpen ? "scale-[1.04]" : "scale-[1.02] group-hover:scale-[1.06]",
            )}
          />
        </span>
      </button>

      {isOpen ? (
        <div
          className={classNames("pointer-events-auto relative flex flex-col gap-2")}
          aria-label={t("presentationDockAriaLabel", { defaultValue: "Presentation slots" })}
        >
          {normalizedPresentation.slots.map((slot) => {
            const card = slot.card;
            const isHighlighted = slot.slot_id === highlightSlotId;
            const hasSlotAttention = !!attentionSlots?.[slot.slot_id];
            const tone = card ? getSlotTone(card.card_type, isDark) : null;
            const title = card
              ? card.title
              : t("presentationSlotEmptyTitle", { defaultValue: "Empty slot" });
            const isHovered = hoveredSlotId === slot.slot_id;
            const placementClassName = getDockPreviewPlacement(
              slot.index,
              normalizedPresentation.slots.length,
            );

            return (
              <div key={slot.slot_id} className="relative flex w-12 justify-end">
                {isHovered ? (
                  <div
                    className={classNames(
                      "pointer-events-none absolute right-[calc(100%+12px)] z-20 w-56 rounded-[18px] border px-3 py-2.5 shadow-[0_24px_60px_-34px_rgba(15,23,42,0.52)] backdrop-blur-2xl transition-all duration-150",
                      placementClassName,
                      isDark
                        ? "border-white/12 bg-slate-950/90 text-slate-100 ring-1 ring-white/6"
                        : "border-white/85 bg-white/94 text-gray-900 ring-1 ring-black/5",
                    )}
                  >
                    <div className="flex items-center gap-2">
                      <span
                        className={classNames(
                          "inline-flex h-6 min-w-[1.5rem] items-center justify-center rounded-full px-1.5 text-[10px] font-semibold",
                          isDark
                            ? "bg-white/[0.08] text-slate-200"
                            : "bg-black/[0.04] text-gray-700",
                        )}
                      >
                        {slot.index}
                      </span>
                      <span
                        className={classNames(
                          "rounded-full px-2 py-1 text-[10px] font-medium uppercase tracking-[0.08em]",
                          card
                            ? isDark
                              ? "bg-white/[0.08] text-white"
                              : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
                            : isDark
                              ? "bg-white/[0.06] text-slate-300"
                              : "bg-black/[0.04] text-gray-600",
                        )}
                      >
                        {card
                          ? getCardTypeLabel(card.card_type, t)
                          : t("presentationSlotEmpty", { defaultValue: "Empty" })}
                      </span>
                    </div>
                    <div
                      className={classNames(
                        "mt-2 max-w-full break-words text-[13px] font-semibold leading-5 [overflow-wrap:anywhere]",
                        isDark ? "text-slate-100" : "text-gray-900",
                      )}
                    >
                      {title}
                    </div>
                    <div
                      className={classNames(
                        "mt-1.5 max-w-full break-words text-[11px] leading-5 [overflow-wrap:anywhere]",
                        isDark ? "text-slate-300" : "text-gray-600",
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
                    {card ? (
                      <div
                        className={classNames(
                          "mt-2 text-[10px]",
                          isDark ? "text-slate-500" : "text-gray-500",
                        )}
                      >
                        {formatUpdatedAt(card.published_at, i18n.language)}
                      </div>
                    ) : null}
                  </div>
                ) : null}

                <button
                  type="button"
                  onMouseEnter={() => setHoveredSlotId(slot.slot_id)}
                  onMouseLeave={() =>
                    setHoveredSlotId((current) => (current === slot.slot_id ? "" : current))
                  }
                  onFocus={() => setHoveredSlotId(slot.slot_id)}
                  onBlur={() =>
                    setHoveredSlotId((current) => (current === slot.slot_id ? "" : current))
                  }
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
                    "group relative flex h-12 w-12 items-center justify-center rounded-[16px] border text-center transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35",
                    card || !readOnly ? "cursor-pointer" : "cursor-default",
                    card
                      ? tone?.buttonClassName
                      : isDark
                        ? "border-dashed border-white/10 bg-white/[0.03] text-slate-100 hover:border-white/16 hover:bg-white/[0.05]"
                        : "border-dashed border-black/10 bg-white/78 text-gray-900 hover:border-black/16 hover:bg-white",
                    !card &&
                      !readOnly &&
                      (isDark ? "hover:border-white/18" : "hover:border-black/14"),
                    isHovered && (isDark ? "translate-x-[-1px]" : "translate-x-[-1px]"),
                    isHighlighted &&
                      (isDark
                        ? "ring-1 ring-[rgb(143,163,187)]/32"
                        : "ring-1 ring-[rgb(62,80,103)]/16"),
                    hasSlotAttention &&
                      (isDark
                        ? "ring-2 ring-cyan-300/70 presentation-slot-attention presentation-slot-attention-dark"
                        : "ring-2 ring-cyan-500/60 presentation-slot-attention presentation-slot-attention-light"),
                  )}
                  aria-label={
                    card
                      ? t("presentationOpenSlot", {
                          index: slot.index,
                          title: card.title,
                          defaultValue: `Open presentation slot ${slot.index}: ${card.title}`,
                        })
                      : t("presentationEmptySlot", {
                          index: slot.index,
                          defaultValue: `Presentation slot ${slot.index} is empty`,
                        })
                  }
                >
                  {card ? (
                    <span
                      className={classNames(
                        "pointer-events-none absolute right-1.5 top-1.5 h-2.5 w-2.5 rounded-full ring-1",
                        tone?.indicatorClassName,
                      )}
                    />
                  ) : null}
                  <span
                    className={classNames(
                      "text-[14px] font-semibold tracking-[0.01em]",
                      card ? "text-current" : isDark ? "text-slate-200" : "text-gray-800",
                    )}
                  >
                    {slot.index}
                  </span>
                </button>
              </div>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
