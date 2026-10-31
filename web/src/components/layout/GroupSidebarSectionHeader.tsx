import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { InfoIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

export function GroupSidebarSectionHeader({
  instanceLabel,
  instanceOrigin,
  action,
}: {
  instanceLabel?: string;
  instanceOrigin?: string | null;
  action?: ReactNode;
}) {
  const { t } = useTranslation("layout");
  return (
    // The action keeps the far end; with a longer label or text size it wraps below.
    <div className="flex shrink-0 min-w-0 flex-wrap items-center gap-x-2 px-4 pt-2 pb-1">
      <span className="text-xs font-medium text-[var(--color-text-tertiary)]">
        {t("workingGroups")}
      </span>
      {instanceLabel && (
        <Popover>
          <PopoverTrigger asChild>
            <IconButton
              variant="ghost"
              size="sm"
              label={t("sidebarInstanceInfo")}
              className="h-6 w-6 text-[var(--color-text-tertiary)]"
            >
              <InfoIcon size={13} strokeWidth={1.25} />
            </IconButton>
          </PopoverTrigger>
          <PopoverContent align="start" className="max-w-[min(20rem,calc(100vw-2rem))] p-3 text-xs">
            <p className="break-words font-medium text-[var(--color-text-primary)]">
              {instanceLabel}
            </p>
            <p className="mt-1 break-all text-[var(--color-text-secondary)]">
              {instanceOrigin || t("connect.noRemoteAccess")}
            </p>
          </PopoverContent>
        </Popover>
      )}
      {action ? <div className="ml-auto flex shrink-0 items-center">{action}</div> : null}
    </div>
  );
}
