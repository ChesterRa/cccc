import { useTranslation } from "react-i18next";
import type { PendingIndicator as Indicator } from "./indicator";

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
