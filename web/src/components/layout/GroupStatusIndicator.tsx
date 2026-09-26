import { useTranslation } from "react-i18next";

import { useSseErrorDetailText } from "../../hooks/useSseErrorDetailText";
import type { GroupStatus, GroupStatusKey } from "../../utils/groupStatus";
import { GROUP_STATUS_DOT_BASE_CLASS } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";

const statusLabelKey: Record<GroupStatusKey, string> = {
  run: "statusRunning",
  paused: "statusPaused",
  idle: "statusIdle",
  stop: "statusStopped",
};

interface GroupStatusIndicatorProps {
  status: GroupStatus;
  variant?: "dot" | "badge";
  className?: string;
  connectionStatus?: "connected" | "connecting" | "disconnected";
}

export function GroupStatusIndicator({
  status,
  variant = "dot",
  className,
  connectionStatus,
}: GroupStatusIndicatorProps) {
  const { t } = useTranslation("layout");
  const label = t(statusLabelKey[status.key]);
  const interrupted = connectionStatus && connectionStatus !== "connected";
  const errorDetail = useSseErrorDetailText();
  const description = interrupted
    ? `${label} · ${t(connectionStatus === "connecting" ? "reconnecting" : "disconnected")}${errorDetail ? ` · ${errorDetail}` : ""}. ${t("connectionInterruptedHint")}`
    : label;
  const dotClass = interrupted
    ? connectionStatus === "connecting"
      ? "bg-amber-400 animate-pulse motion-reduce:animate-none"
      : "bg-rose-500"
    : status.dotClass;

  if (variant === "dot") {
    return (
      <span
        className={classNames(GROUP_STATUS_DOT_BASE_CLASS, dotClass, className)}
        role="img"
        aria-label={description}
        title={description}
      />
    );
  }

  return (
    <span
      className={classNames(
        "inline-flex min-h-6 shrink-0 items-center gap-1.5 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2 text-xs font-medium text-[var(--color-text-secondary)]",
        className,
      )}
      title={description}
      aria-label={description}
      role={connectionStatus ? "status" : undefined}
      data-connection-state={connectionStatus}
    >
      <span className={classNames(GROUP_STATUS_DOT_BASE_CLASS, dotClass)} aria-hidden="true" />
      <span>{label}</span>
    </span>
  );
}
