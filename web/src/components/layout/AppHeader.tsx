import { useUIStore } from "../../stores/useUIStore";
import { useContext, type Ref } from "react";
import { useTranslation } from "react-i18next";
import { GROUP_STATUS_LABEL_KEYS, getGroupStatusFromSource } from "../../utils/groupStatus";
import { GroupDoc, GroupRuntimeStatus, TextScale, Theme } from "../../types";
import { ChevronDownIcon, ClipboardIcon, EditIcon, SearchIcon, MoreIcon, MenuIcon } from "../Icons";
import { IconButton } from "../ui/icon-button";
import { GroupStatusIndicator } from "./GroupStatusIndicator";
import { useSseErrorDetailText } from "../../hooks/useSseErrorDetailText";
import { groupRunMenuActions } from "./groupRunMenuActions";
import { useGroupMenu } from "./useGroupMenu";
import type { GroupControl } from "../../utils/groupControls";
import { AppSettingsMenu } from "./AppSettingsMenu";
import { PendingSummaryContext } from "../../features/pendingReplies/context";
import { aggregatePendingIndicator } from "../../features/pendingReplies/indicator";
import { PendingIndicatorText } from "../../features/pendingReplies/PendingIndicator";

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
  mobileReadingHidden?: boolean;
  mobileReadingOverlay?: boolean;
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
  mobileReadingHidden = false,
  mobileReadingOverlay = false,
}: AppHeaderProps) {
  const { t } = useTranslation("layout");
  const pending = useContext(PendingSummaryContext);
  const showPendingPrompts = useUIStore((state) => state.showPendingPrompts);
  const totalPending =
    pending && showPendingPrompts
      ? aggregatePendingIndicator(pending.summaries, Object.keys(pending.summaries))
      : null;
  const controlsBusy = useUIStore((state) => state.busy.startsWith("group-"));
  const isSmallScreen = useUIStore((state) => state.isSmallScreen);
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
  const runLabel = selectedStatus
    ? t("groupRun.control", {
        group: groupTitle,
        state: t(GROUP_STATUS_LABEL_KEYS[selectedStatus.key]),
      })
    : "";
  const sseErrorDetail = useSseErrorDetailText();
  const statusControlLabel =
    sseStatus === "connected"
      ? runLabel
      : `${runLabel} · ${t(sseStatus === "connecting" ? "reconnecting" : "disconnected")}${sseErrorDetail ? ` · ${sseErrorDetail}` : ""}. ${t("connectionInterruptedHint")}`;
  const runMenu = useGroupMenu(
    runLabel,
    selectedStatus && onControlGroup && !webReadOnly
      ? groupRunMenuActions(
          selectedStatus.key,
          t,
          (control) => onControlGroup(selectedGroupId, control),
          controlsBusy,
        )
      : [],
  );
  const workShortcuts = (
    <div
      className="hidden min-w-0 flex-1 basis-0 items-center justify-end @min-[760px]/group-header:flex"
      data-group-work-shortcuts-host
    >
      {!webReadOnly && (
        <div
          className="flex shrink-0 items-center gap-0.5 border-r border-[var(--glass-border-subtle)] pr-2"
          data-group-work-shortcuts
        >
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={onOpenSearch}
            disabled={!selectedGroupId}
            className="text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            label={t("searchMessages")}
          >
            <SearchIcon size={18} strokeWidth={1.25} />
          </IconButton>
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={onOpenContext}
            disabled={!selectedGroupId}
            className="text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)]"
            label={t("context")}
          >
            <ClipboardIcon size={18} strokeWidth={1.25} />
          </IconButton>
        </div>
      )}
    </div>
  );
  return (
    <header
      className={`@container/group-header absolute inset-x-0 top-0 z-20 flex h-[48px] shrink-0 items-center px-[12px] glass-header transition-transform duration-150 motion-reduce:transition-none md:relative md:inset-auto md:h-11 md:px-3 ${mobileReadingHidden ? "pointer-events-none -translate-y-full" : ""}`}
      inert={mobileReadingHidden || undefined}
      aria-hidden={mobileReadingHidden || undefined}
      data-mobile-reading-hidden={mobileReadingHidden || undefined}
      style={
        mobileReadingOverlay
          ? {
              backgroundColor: "var(--color-bg-primary)",
              backgroundImage: "linear-gradient(var(--color-chat-bg), var(--color-chat-bg))",
            }
          : undefined
      }
    >
      <div
        className="@container/group-work-header flex min-w-0 flex-1 items-center gap-1 pr-1 md:gap-2 md:pr-3"
        data-group-header-work
      >
        <div
          className="flex min-w-0 flex-1 basis-0 items-center gap-1 md:gap-2"
          data-group-header-identity
        >
          <IconButton
            type="button"
            variant="secondary"
            className="relative -ml-1 h-[44px] w-[44px] text-[var(--color-text-secondary)] md:hidden"
            onClick={onOpenSidebar}
            data-sidebar-toggle="true"
            label={
              totalPending
                ? `${t("openSidebar")} · ${t("pendingRepliesIndicator", { value: totalPending.text })}`
                : t("openSidebar")
            }
          >
            <MenuIcon size={18} />
            {totalPending && (
              <span className="absolute -right-1 -top-1">
                <PendingIndicatorText indicator={totalPending} />
              </span>
            )}
          </IconButton>
          <div className="min-w-0">
            <h1
              className="min-w-0 truncate text-base font-semibold leading-tight text-[var(--color-text-primary)]"
              title={groupTitle}
              data-group-title
            >
              {groupTitle}
            </h1>
          </div>
          {canEditGroup && (
            <IconButton
              type="button"
              variant="ghost"
              size="sm"
              className="hidden shrink-0 text-[var(--color-text-tertiary)] hover:text-[var(--color-text-primary)] @min-[480px]/group-work-header:inline-flex"
              label={t("editGroup")}
              aria-haspopup="dialog"
              data-group-title-edit
              onClick={onOpenGroupEdit}
            >
              <EditIcon size={18} strokeWidth={1.25} />
            </IconButton>
          )}
          {selectedStatus && (
            <span className="inline-flex shrink-0">
              {runMenu.available ? (
                <button
                  type="button"
                  data-group-run-controls
                  data-group-run-control={selectedGroupId}
                  aria-label={statusControlLabel}
                  title={statusControlLabel}
                  className={`inline-flex min-h-8 min-w-8 items-center justify-center rounded-full pointer-coarse:min-h-[44px] pointer-coarse:min-w-[44px] md:pointer-coarse:min-h-10 md:pointer-coarse:min-w-10 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-text-secondary)] relative`}
                  aria-haspopup="menu"
                  aria-expanded={runMenu.open}
                  onClick={(event) => runMenu.toggle(event.currentTarget)}
                  onKeyDown={(event) => {
                    if (!runMenu.open && (event.key === "ArrowDown" || event.key === "ArrowUp")) {
                      event.preventDefault();
                      runMenu.toggle(event.currentTarget);
                    } else runMenu.onKeyDown(event);
                  }}
                >
                  {/* The state symbol shows alone; the word stays in the tooltip. Its
                      frame and ▾ stay at every width: a bare ▶ reads as a play key. */}
                  <GroupStatusIndicator
                    status={selectedStatus}
                    connectionStatus={sseStatus}
                    variant="badge"
                    className="min-h-8 min-w-8 justify-center pointer-coarse:min-h-[44px] pointer-coarse:min-w-[44px] md:pointer-coarse:min-h-10 md:pointer-coarse:min-w-10 [&>span:last-child]:hidden cursor-pointer transition-colors hover:text-[var(--color-text-primary)] pr-6"
                  />
                  <ChevronDownIcon
                    size={13}
                    aria-hidden="true"
                    className="pointer-events-none absolute right-2 text-[var(--color-text-tertiary)]"
                  />
                </button>
              ) : (
                <GroupStatusIndicator
                  status={selectedStatus}
                  connectionStatus={sseStatus}
                  variant="badge"
                />
              )}
            </span>
          )}
          {runMenu.menu}
        </div>

        <div
          ref={workControlsRef}
          className="flex shrink-0 items-center gap-1"
          data-group-work-controls-host
        />
        {!isSmallScreen ? (
          // The tools take the right half, as wide as the identity on the left, so the
          // view switch between them stays centred.
          <div
            className="flex min-w-0 flex-1 basis-0 items-center justify-end gap-2"
            data-lcd-header-end
          >
            {workShortcuts}
          </div>
        ) : (
          workShortcuts
        )}
      </div>
      <div
        className="flex shrink-0 items-center justify-end gap-1 md:gap-2 md:min-w-[max(7.5rem,calc(var(--group-side-panel-width,0px)-1rem+4px))]"
        data-group-header-resources
      >
        <div
          ref={sidePanelControlsRef}
          className="flex shrink-0 items-center gap-0.5"
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
              className="shrink-0 max-md:h-[44px] max-md:w-[44px] text-[var(--color-text-secondary)] @min-[760px]/group-header:hidden"
              onClick={onOpenMobileMenu}
              label={t("menu")}
              data-group-header-menu
            >
              <MoreIcon size={18} />
            </IconButton>
          </>
        )}
      </div>
    </header>
  );
}
