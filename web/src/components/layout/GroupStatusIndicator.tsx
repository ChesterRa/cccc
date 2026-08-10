import { useTranslation } from "react-i18next";

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
}

export function GroupStatusIndicator({
  status,
  variant = "dot",
  className,
}: GroupStatusIndicatorProps) {
  const { t } = useTranslation("layout");
  const label = t(statusLabelKey[status.key]);

  if (variant === "dot") {
    return (
      <span
        className={classNames(GROUP_STATUS_DOT_BASE_CLASS, status.dotClass, className)}
        role="img"
        aria-label={label}
        title={label}
      />
    );
  }

  return (
    <span
      className={classNames(
        "inline-flex min-h-6 shrink-0 items-center gap-1.5 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2 text-[11px] font-medium text-[var(--color-text-secondary)]",
        className,
      )}
      title={label}
    >
      <span
        className={classNames(GROUP_STATUS_DOT_BASE_CLASS, status.dotClass)}
        aria-hidden="true"
      />
      <span>{label}</span>
    </span>
  );
}
