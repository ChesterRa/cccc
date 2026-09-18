import type { Ref } from "react";
import { useTranslation } from "react-i18next";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { GroupDoc, GroupRuntimeStatus, TextScale, Theme } from "../../types";
import { ClipboardIcon, EditIcon, SearchIcon, MoreIcon, MenuIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { GroupStatusIndicator } from "./GroupStatusIndicator";
import { groupRunMenuActions } from "./groupRunMenuActions";
import { useGroupMenu } from "./useGroupMenu";
import type { GroupControl } from "../../utils/groupControls";
import { AppSettingsMenu } from "./AppSettingsMenu";

export interface AppHeaderProps {
  theme: Theme;
  textScale: TextScale;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
  webReadOnly?: boolean;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  selectedGroupRuntimeStatus: GroupRuntimeStatus | null;
  sseStatus: "connected" | "connecting" | "disconnected";
  onOpenSidebar: () => void;
  onOpenGroupEdit?: () => void;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  /** Opens the Group run menu from the status badge; absent when the viewer cannot control Groups. */
  onControlGroup?: (groupId: string, control: GroupControl) => void;
  onOpenSettings: () => void;
  canAccessAccount: boolean;
  accountLabel?: string | null;
  onOpenAccount: () => void;
  onOpenMobileMenu: () => void;
  workControlsRef?: Ref<HTMLDivElement>;
  sidePanelControlsRef?: Ref<HTMLDivElement>;
}

export function AppHeader({
  theme,
  textScale,
  onThemeChange,
  onTextScaleChange,
  webReadOnly,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  selectedGroupRuntimeStatus,
  onOpenSidebar,
  onOpenGroupEdit,
  onOpenSearch,
  onOpenContext,
  onControlGroup,
  onOpenSettings,
  canAccessAccount,
  accountLabel,
  onOpenAccount,
  onOpenMobileMenu,
  sseStatus,
  workControlsRef,
  sidePanelControlsRef,
}: AppHeaderProps) {
  const { t } = useTranslation("layout");
  const groupTitle = groupDoc?.title || (selectedGroupId ? selectedGroupId : t("selectGroup"));
  const canEditGroup = !!selectedGroupId && !webReadOnly && !!onOpenGroupEdit;
  const selectedStatus = selectedGroupId
    ? getGroupStatusFromSource({
        running: selectedGroupRunning,
        state:
          (selectedGroupRuntimeStatus?.lifecycle_state as GroupDoc["state"] | undefined) ||
          groupDoc?.state,
        runtime_status: selectedGroupRuntimeStatus || undefined,
      })
    : null;
  const runMenu = useGroupMenu(
    groupTitle,
    selectedStatus && onControlGroup && !webReadOnly
      ? groupRunMenuActions(selectedStatus.key, t, (control) =>
          onControlGroup(selectedGroupId, control),
        )
      : [],
  );
  return (
    <header className="@container/group-header absolute inset-x-0 top-0 z-20 flex h-14 shrink-0 items-center gap-2 px-3 glass-header md:relative md:inset-auto md:px-4">
      <div
        className="flex min-w-0 flex-1 items-center gap-2 @min-[760px]/group-header:max-w-[28cqw] @min-[760px]/group-header:flex-initial"
        data-group-header-identity
      >
        <IconButton
          type="button"
          variant="secondary"
          className="-ml-1 text-[var(--color-text-secondary)] md:hidden"
          onClick={onOpenSidebar}
          data-sidebar-toggle="true"
          label={t("openSidebar")}
        >
          <MenuIcon size={18} />
        </IconButton>
        <div className="min-w-0">
          <h1
            className="min-w-0 truncate text-base font-semibold leading-tight text-[var(--color-text-primary)] md:text-[1.125rem]"
            title={groupTitle}
          >
            {groupTitle}
          </h1>
          {!!selectedGroupId && sseStatus !== "connected" && (
            <p
              role="status"
              className="hidden truncate text-xs leading-tight text-[var(--color-text-secondary)] @min-[760px]/group-header:block"
              title={t("connectionInterruptedHint")}
            >
              {sseStatus === "connecting" ? t("reconnecting") : t("disconnected")}
            </p>
          )}
        </div>
        {canEditGroup && (
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            className="hidden shrink-0 text-[var(--color-text-tertiary)] @min-[480px]/group-header:inline-flex"
            label={t("editGroup")}
            aria-haspopup="dialog"
            data-group-title-edit
            onClick={onOpenGroupEdit}
          >
            <EditIcon size={16} />
          </IconButton>
        )}
        {!!selectedGroupId && sseStatus !== "connected" && (
          <span
            role="status"
            className={`h-2 w-2 shrink-0 rounded-full @min-[760px]/group-header:hidden ${sseStatus === "connecting" ? "bg-amber-400 animate-pulse" : "bg-rose-500"}`}
            title={`${sseStatus === "connecting" ? t("reconnecting") : t("disconnected")}. ${t("connectionInterruptedHint")}`}
          >
            <span className="sr-only">
              {sseStatus === "connecting" ? t("reconnecting") : t("disconnected")}
            </span>
          </span>
        )}
        {selectedStatus && (
          <span className="hidden shrink-0 @min-[480px]/group-header:inline-flex">
            {runMenu.available ? (
              <button
                type="button"
                data-group-run-controls
                className="rounded-full focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-text-secondary)]"
                aria-haspopup="menu"
                aria-expanded={runMenu.open}
                onClick={(event) => runMenu.toggle(event.currentTarget)}
                onKeyDown={runMenu.onKeyDown}
              >
                <GroupStatusIndicator
                  status={selectedStatus}
                  variant="badge"
                  className="cursor-pointer transition-colors hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
                />
              </button>
            ) : (
              <GroupStatusIndicator status={selectedStatus} variant="badge" />
            )}
          </span>
        )}
        {runMenu.menu}
      </div>

      <div
        ref={workControlsRef}
        className="flex shrink-0 items-center gap-1 @min-[760px]/group-header:ml-4"
        data-group-work-controls-host
      />
      {!webReadOnly && (
        <div
          className="hidden shrink-0 items-center gap-0.5 @min-[760px]/group-header:flex"
          data-group-work-shortcuts
        >
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={onOpenSearch}
            disabled={!selectedGroupId}
            className="text-[var(--color-text-secondary)]"
            label={t("searchMessages")}
          >
            <SearchIcon size={17} />
          </IconButton>
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={onOpenContext}
            disabled={!selectedGroupId}
            className="text-[var(--color-text-secondary)]"
            label={t("context")}
          >
            <ClipboardIcon size={17} />
          </IconButton>
        </div>
      )}
      <div
        ref={sidePanelControlsRef}
        className="ml-auto flex shrink-0 items-center gap-0.5"
        data-group-side-panel-controls-host
      />
      {!webReadOnly && (
        <>
          <div className="hidden shrink-0 border-l border-[var(--glass-border-subtle)] pl-2 @min-[760px]/group-header:block">
            <AppSettingsMenu
              key={selectedGroupId}
              theme={theme}
              textScale={textScale}
              onThemeChange={onThemeChange}
              onTextScaleChange={onTextScaleChange}
              canAccessAccount={canAccessAccount}
              accountLabel={accountLabel}
              canOpenSettings={Boolean(selectedGroupId) || canAccessAccount}
              onOpenAccount={onOpenAccount}
              onOpenSettings={onOpenSettings}
            />
          </div>
          <IconButton
            type="button"
            variant="secondary"
            className="shrink-0 text-[var(--color-text-secondary)] @min-[760px]/group-header:hidden"
            onClick={onOpenMobileMenu}
            label={t("menu")}
          >
            <MoreIcon size={18} />
          </IconButton>
        </>
      )}
    </header>
  );
}
