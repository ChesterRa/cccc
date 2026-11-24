import { useTranslation } from "react-i18next";
import { GROUP_STATUS_LABEL_KEYS, type GroupStatus } from "../../utils/groupStatus";
import { classNames } from "../../utils/classNames";

// Running and idle Groups read plainly; paused ones dim, stopped and offline ones
// dim further. Hover keeps the tone so the state stays readable under the pointer.
const NAME_TONE: Record<GroupStatus["key"], string> = {
  run: "text-[var(--color-text-primary)]",
  idle: "text-[var(--color-text-primary)]",
  unknown: "text-[var(--color-text-primary)]",
  paused: "text-[var(--color-text-secondary)]",
  stop: "text-[var(--color-text-muted)]",
  offline: "text-[var(--color-text-muted)]",
};

interface GroupSidebarNameProps {
  title: string;
  status: GroupStatus;
  isActive: boolean;
}

/**
 * A sidebar row's Group name carries its run state instead of a separate symbol.
 * Only an unknown state, which needs attention, keeps a mark. The state is always
 * named in text for assistive technology and in the tooltip, never by tone alone.
 * The open Group's row is drawn in reverse; its state is shown in the header.
 */
export function GroupSidebarName({ title, status, isActive }: GroupSidebarNameProps) {
  const { t } = useTranslation("layout");
  const label = t(GROUP_STATUS_LABEL_KEYS[status.key]);
  return (
    <span className="flex min-w-0 items-center gap-1.5" data-group-status={status.key}>
      <span
        data-group-name
        className={classNames(
          "text-sm truncate",
          isActive
            ? "font-semibold text-[rgb(35,36,37)] dark:text-white"
            : classNames("font-normal", NAME_TONE[status.key]),
        )}
        title={`${title} · ${label}`}
      >
        {title}
      </span>
      {status.key === "unknown" && (
        <span aria-hidden="true" className="shrink-0 text-xs text-[var(--color-text-secondary)]">
          ?
        </span>
      )}
      <span className="sr-only">{label}</span>
    </span>
  );
}
