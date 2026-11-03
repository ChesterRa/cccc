import { useVisiblePendingSummary } from "./useVisiblePendingSummary";
import { useTranslation } from "react-i18next";
import { pendingIndicator } from "./indicator";

export function usePendingGroupLabel(groupId: string, title: string): string {
  const context = useVisiblePendingSummary();
  const { t } = useTranslation("layout");
  if (!context) return title;
  const indicator = pendingIndicator(context.summaries[groupId]);
  if (!indicator) return title;
  const status =
    indicator.state === "fresh"
      ? t("pendingRepliesCount", { value: indicator.text })
      : t(
          indicator.state === "stale"
            ? "pendingRepliesStale"
            : indicator.daemonUnsupported
              ? "pendingRepliesDaemonUnsupported"
              : "pendingRepliesUnknown",
        );
  return `${title} · ${status}`;
}
