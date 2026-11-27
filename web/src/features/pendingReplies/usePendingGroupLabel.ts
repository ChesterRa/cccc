import { useVisiblePendingSummary } from "./useVisiblePendingSummary";
import { useTranslation } from "react-i18next";
import { pendingIndicator, pendingIndicatorStatusLabel } from "./indicator";

export function usePendingGroupLabel(groupId: string, title: string): string {
  const context = useVisiblePendingSummary();
  const { t } = useTranslation("layout");
  if (!context) return title;
  const indicator = pendingIndicator(context.summaries[groupId]);
  if (!indicator) return title;
  return `${title} · ${pendingIndicatorStatusLabel(t, indicator)}`;
}
