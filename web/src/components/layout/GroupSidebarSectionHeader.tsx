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
    <div className="@container/sidebar-section shrink-0 min-w-0 px-3 pt-2 pb-1 [@media(pointer:coarse)]:px-2">
      <div className="flex min-w-0 items-center gap-1">
        <div className="flex min-w-0 items-center gap-1">
          <span className="sr-only">{t("workingGroups")}</span>
          <span
            aria-hidden="true"
            data-sidebar-section-label
            className="min-w-0 truncate text-xs font-medium text-[var(--color-text-tertiary)]"
            title={t("workingGroups")}
          >
            <span className="@[13.75rem]/sidebar-section:hidden">{t("workingGroupsShort")}</span>
            <span className="hidden @[13.75rem]/sidebar-section:inline">{t("workingGroups")}</span>
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
              <PopoverContent
                align="start"
                className="max-w-[min(20rem,calc(100vw-2rem))] p-3 text-xs"
              >
                <p className="break-words font-medium text-[var(--color-text-primary)]">
                  {instanceLabel}
                </p>
                <p className="mt-1 break-all text-[var(--color-text-secondary)]">
                  {instanceOrigin || t("connect.noRemoteAccess")}
                </p>
              </PopoverContent>
            </Popover>
          )}
        </div>
        {action ? <div className="ml-auto flex shrink-0 items-center">{action}</div> : null}
      </div>
    </div>
  );
}
