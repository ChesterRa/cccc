import { useContext, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { PendingSummaryContext } from "../pendingReplies/context";
import { useUiStylePrefs } from "../../utils/uiStyle";
import { useChetDecisionsStore } from "./chetDecisionsStore";

/**
 * The way into the decision queue from Chet's conversation: how many questions wait across
 * all Groups. A preview-style feature for now; hidden while nothing is known to wait.
 */
export function ChetDecisionsEntry() {
  const { t } = useTranslation("modals");
  const { style } = useUiStylePrefs();
  const summary = useContext(PendingSummaryContext);
  const setOpen = useChetDecisionsStore((state) => state.setOpen);
  const { total, unknown } = useMemo(() => {
    let count = 0;
    let unread = false;
    for (const entry of Object.values(summary?.summaries ?? {})) {
      if (typeof entry.pending === "number") count += entry.pending;
      else unread = true;
    }
    return { total: count, unknown: unread };
  }, [summary?.summaries]);
  // Shown only while something is known to wait. A Group whose count cannot be read adds a
  // "+" to a real count, but never keeps an empty queue's entry on screen by itself.
  if (style !== "preview" || !summary || total === 0) return null;
  return (
    <button
      type="button"
      onClick={() => setOpen(true)}
      className="flex flex-none items-center gap-2 border-b border-[var(--glass-border-subtle)] px-4 py-2 text-left text-sm hover:bg-[var(--glass-tab-bg-hover)] sm:px-6"
      data-chet-decisions-entry
    >
      <span className="mx-auto flex w-full max-w-3xl items-center gap-2">
        <span className="font-semibold text-[var(--color-text-primary)]">
          {t("chetDecisions.title")}
        </span>
        <span className="tabular-nums text-[var(--color-text-secondary)]">
          {t("chetDecisions.entryCount", { count: total })}
          {unknown ? "+" : ""}
        </span>
        <span className="ml-auto text-xs text-[var(--color-text-tertiary)]">
          {t("chetDecisions.entryHint")}
        </span>
      </span>
    </button>
  );
}
