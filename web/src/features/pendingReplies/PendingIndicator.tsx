import { useContext } from "react";
import { useTranslation } from "react-i18next";
import { PendingReplyNavigationContext, PendingSummaryContext } from "./context";
import { pendingIndicator, type PendingIndicator as Indicator } from "./indicator";

export function PendingIndicatorText({ indicator }: { indicator: Indicator }) {
  const { t } = useTranslation("layout");
  const label =
    indicator.state === "fresh"
      ? t("pendingRepliesCount", { value: indicator.text })
      : indicator.state === "stale"
        ? t("pendingRepliesStale")
        : indicator.daemonUnsupported
          ? t("pendingRepliesDaemonUnsupported")
          : t("pendingRepliesUnknown");
  return (
    <span
      title={label}
      aria-label={label}
      className="inline-flex min-w-[20px] justify-center rounded-full bg-[var(--color-accent-primary)] px-1.5 py-0.5 text-[10px] font-semibold leading-4 text-[var(--color-bg-primary)]"
      data-pending-state={indicator.state}
    >
      {indicator.text}
    </span>
  );
}

/** A separate target from the row prevents a badge click starting a reorder. */
export function GroupPendingIndicator({
  groupId,
  groupName,
  collapsed = false,
}: {
  groupId: string;
  groupName?: string;
  collapsed?: boolean;
}) {
  const context = useContext(PendingSummaryContext);
  const open = useContext(PendingReplyNavigationContext);
  const { t } = useTranslation("layout");
  if (!context) return null;
  const indicator = pendingIndicator(context.summaries[groupId]);
  if (!indicator) return null;
  if (collapsed || !open)
    return (
      <span className="absolute -right-1 -top-1">
        <PendingIndicatorText indicator={indicator} />
      </span>
    );
  const actionLabel =
    indicator.state === "fresh"
      ? t("openPendingRepliesCount", { value: indicator.text })
      : indicator.state === "stale"
        ? t("openPendingRepliesStale")
        : indicator.daemonUnsupported
          ? t("openPendingRepliesDaemonUnsupported")
          : t("openPendingRepliesUnknown");
  const label = `${groupName || groupId} · ${actionLabel}`;
  return (
    <button
      type="button"
      className="flex h-[44px] min-w-[44px] shrink-0 items-center justify-center rounded-md focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--color-accent-primary)]"
      aria-label={label}
      title={label}
      onPointerDown={(event) => event.stopPropagation()}
      onKeyDown={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        open(groupId);
      }}
    >
      <PendingIndicatorText indicator={indicator} />
    </button>
  );
}
