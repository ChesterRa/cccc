import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { Actor, GroupDoc, GroupRuntimeStatus, TextScale, Theme } from "../../types";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import {
  getGroupControlVisual,
  getLaunchControlMode,
  resolveGroupControls,
} from "../../utils/groupControls";
import { classNames } from "../../utils/classNames";
import { TextScaleSwitcher } from "../TextScaleSwitcher";
import { ThemeToggleCompact } from "../ThemeToggle";
import { LanguageSwitcher } from "../LanguageSwitcher";
import {
  ClipboardIcon,
  SearchIcon,
  PlayIcon,
  PauseIcon,
  StopIcon,
  SettingsIcon,
  AccountIcon,
  EditIcon,
  MoreIcon,
  MenuIcon,
} from "../Icons";
import { IconButton } from "../ui/icon-button";
import { GroupStatusIndicator } from "./GroupStatusIndicator";

export interface AppHeaderProps {
  isDark: boolean;
  theme: Theme;
  textScale: TextScale;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
  webReadOnly?: boolean;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  selectedGroupRuntimeStatus: GroupRuntimeStatus | null;
  actors: Actor[];
  sseStatus: "connected" | "connecting" | "disconnected";
  busy: string;
  onOpenSidebar: () => void;
  onOpenGroupEdit?: () => void;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSetGroupState: (state: "active" | "paused" | "idle") => void | Promise<void>;
  onOpenSettings: () => void;
  canAccessAccount: boolean;
  onOpenAccount: () => void;
  onOpenMobileMenu: () => void;
}

export function AppHeader({
  isDark,
  theme,
  textScale,
  onThemeChange,
  onTextScaleChange,
  webReadOnly,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  selectedGroupRuntimeStatus,
  actors,
  busy,
  onOpenSidebar,
  onOpenGroupEdit,
  onOpenSearch,
  onOpenContext,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
  onOpenSettings,
  canAccessAccount,
  onOpenAccount,
  onOpenMobileMenu,
  sseStatus,
}: AppHeaderProps) {
  const { t } = useTranslation("layout");
  const [pendingToggleAction, setPendingToggleAction] = useState<"launch" | "pause" | null>(null);
  const [hasObservedGroupBusy, setHasObservedGroupBusy] = useState(false);
  const headerRailClass = "flex items-center gap-1 p-[3px]";
  const headerUtilityRailClass = "flex items-center gap-0.5 p-[3px]";
  const headerUtilityButtonClass =
    "flex items-center justify-center h-8 w-8 rounded-xl transition-all duration-150 active:scale-[0.95] shrink-0 border border-transparent bg-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]";
  const headerRailDividerClass = "mx-1 h-5 w-px bg-[var(--glass-border-subtle)]";
  const selectedStatus = selectedGroupId
    ? getGroupStatusFromSource({
        running: selectedGroupRunning,
        state:
          (selectedGroupRuntimeStatus?.lifecycle_state as GroupDoc["state"] | undefined) ||
          groupDoc?.state,
        runtime_status: selectedGroupRuntimeStatus || undefined,
      })
    : null;
  const selectedStatusKey = selectedStatus?.key ?? null;
  const launchMode = getLaunchControlMode(selectedStatusKey);
  const launchControl = getGroupControlVisual(selectedStatusKey, "launch", busy);
  const pauseControl = getGroupControlVisual(selectedStatusKey, "pause", busy);
  const stopControl = getGroupControlVisual(selectedStatusKey, "stop", busy);
  const {
    launchHardUnavailable,
    pauseHardUnavailable,
    stopHardUnavailable,
    launchDisabled,
    pauseDisabled,
    stopDisabled,
  } = resolveGroupControls({
    selectedGroupId,
    actorCount: actors.length,
    statusKey: selectedStatusKey,
    busy,
  });
  const isPauseAction = selectedStatusKey === "run";
  const toggleControl = isPauseAction ? pauseControl : launchControl;
  const toggleDisabled =
    (isPauseAction ? pauseDisabled : launchDisabled) || pendingToggleAction !== null;
  const toggleHardUnavailable = isPauseAction ? pauseHardUnavailable : launchHardUnavailable;
  const toggleTitle = isPauseAction
    ? t("pauseDelivery")
    : launchMode === "activate"
      ? t("resumeDelivery")
      : t("launchAllAgents");
  const isGroupBusy = busy.startsWith("group-");

  useEffect(() => {
    if (!pendingToggleAction) return;
    let timerId: number | null = null;
    const resetPendingState = () => {
      timerId = window.setTimeout(() => {
        setPendingToggleAction(null);
        setHasObservedGroupBusy(false);
      }, 0);
    };

    if (selectedGroupId.trim() === "") {
      resetPendingState();
      return () => {
        if (timerId !== null) window.clearTimeout(timerId);
      };
    }
    if (isGroupBusy) {
      if (!hasObservedGroupBusy) {
        timerId = window.setTimeout(() => {
          setHasObservedGroupBusy(true);
        }, 0);
      }
      return () => {
        if (timerId !== null) window.clearTimeout(timerId);
      };
    }
    const launchSettled =
      pendingToggleAction === "launch" &&
      (selectedStatusKey === "run" || selectedStatusKey === "idle");
    const pauseSettled = pendingToggleAction === "pause" && selectedStatusKey === "paused";
    if (launchSettled || pauseSettled || hasObservedGroupBusy) {
      resetPendingState();
    }
    return () => {
      if (timerId !== null) window.clearTimeout(timerId);
    };
  }, [pendingToggleAction, hasObservedGroupBusy, isGroupBusy, selectedGroupId, selectedStatusKey]);

  const handleLaunchClick = () => {
    if (launchDisabled || selectedStatusKey === "run") return;
    setPendingToggleAction("launch");
    setHasObservedGroupBusy(false);
    if (launchMode === "activate") {
      void onSetGroupState("active");
      return;
    }
    onStartGroup();
  };

  const handlePauseClick = () => {
    if (pauseDisabled || selectedStatusKey === "paused") return;
    setPendingToggleAction("pause");
    setHasObservedGroupBusy(false);
    void onSetGroupState("paused");
  };

  const handleStopClick = () => {
    if (stopDisabled || selectedStatusKey === "stop") return;
    onStopGroup();
  };

  const handleToggleClick = () => {
    if (isPauseAction) {
      handlePauseClick();
      return;
    }
    handleLaunchClick();
  };
  return (
    <header className="absolute inset-x-0 top-0 z-20 flex h-14 flex-shrink-0 items-center justify-between gap-3 px-4 glass-header md:relative md:inset-auto md:px-5">
      <div className="flex min-w-0 items-center gap-2">
        <IconButton
          type="button"
          variant="secondary"
          className="-ml-1 text-[var(--color-text-secondary)] md:hidden"
          onClick={onOpenSidebar}
          label={t("openSidebar")}
        >
          <MenuIcon size={18} />
        </IconButton>

        <div className="min-w-0 flex items-center gap-2">
          <div className="flex min-w-0 items-center gap-1.5">
            <h1 className="truncate text-base font-semibold leading-tight text-[var(--color-text-primary)] md:text-[1.125rem]">
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : t("selectGroup"))}
            </h1>
            {selectedGroupId && sseStatus !== "connected" && (
              <span
                className={classNames(
                  "h-2 w-2 flex-shrink-0 rounded-full",
                  sseStatus === "connecting" ? "bg-amber-400 animate-pulse" : "bg-rose-500",
                )}
                title={sseStatus === "connecting" ? t("reconnecting") : t("disconnected")}
              />
            )}
            {selectedStatus && <GroupStatusIndicator status={selectedStatus} variant="badge" />}
          </div>

          {selectedGroupId && !webReadOnly && onOpenGroupEdit && (
            <IconButton
              type="button"
              variant="ghost"
              size="sm"
              className="hidden text-[var(--color-text-tertiary)] md:inline-flex"
              onClick={onOpenGroupEdit}
              label={t("editGroup")}
            >
              <EditIcon size={14} />
            </IconButton>
          )}
        </div>
      </div>

      {/* Right Actions */}
      <div className="flex items-center gap-1.5">
        {!webReadOnly && (
          <>
            {/* Desktop Actions */}
            <div className="mr-1 hidden items-center gap-1.5 md:flex">
              <div className={headerRailClass}>
                <IconButton
                  type="button"
                  variant="ghost"
                  size="rail"
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
                  size="rail"
                  onClick={onOpenContext}
                  disabled={!selectedGroupId}
                  className="text-[var(--color-text-secondary)]"
                  label={t("context")}
                >
                  <ClipboardIcon size={17} />
                </IconButton>
                <span className={headerRailDividerClass} aria-hidden="true" />
                <IconButton
                  type="button"
                  variant="ghost"
                  onClick={handleToggleClick}
                  disabled={toggleDisabled}
                  className={classNames(
                    toggleControl.className,
                    toggleHardUnavailable && "opacity-45",
                  )}
                  label={toggleTitle}
                  aria-pressed={toggleControl.active}
                >
                  {isPauseAction ? <PauseIcon size={17} /> : <PlayIcon size={17} />}
                </IconButton>

                <IconButton
                  type="button"
                  variant="ghost"
                  onClick={handleStopClick}
                  disabled={stopDisabled}
                  className={classNames(stopControl.className, stopHardUnavailable && "opacity-45")}
                  label={t("stopAllAgents")}
                  aria-pressed={stopControl.active}
                >
                  <StopIcon size={17} />
                </IconButton>
              </div>

              <div className={headerUtilityRailClass}>
                <ThemeToggleCompact
                  theme={theme}
                  onThemeChange={onThemeChange}
                  isDark={isDark}
                  variant="rail"
                  className={headerUtilityButtonClass}
                />
                <TextScaleSwitcher
                  textScale={textScale}
                  onTextScaleChange={onTextScaleChange}
                  variant="rail"
                  className={headerUtilityButtonClass}
                />
                <LanguageSwitcher
                  isDark={isDark}
                  variant="rail"
                  className={classNames(
                    headerUtilityButtonClass,
                    "text-[10px] font-semibold tracking-[0.04em]",
                  )}
                />
                <span
                  className="mx-0.5 h-4 w-px bg-[var(--glass-border-subtle)]"
                  aria-hidden="true"
                />
                {canAccessAccount ? (
                  <IconButton
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={onOpenAccount}
                    className={headerUtilityButtonClass}
                    label={t("account")}
                  >
                    <AccountIcon size={17} />
                  </IconButton>
                ) : null}
                <IconButton
                  type="button"
                  variant="ghost"
                  size="sm"
                  onClick={onOpenSettings}
                  disabled={!selectedGroupId && !canAccessAccount}
                  className={classNames(
                    headerUtilityButtonClass,
                    "disabled:opacity-45 disabled:text-[var(--color-text-tertiary)]",
                  )}
                  label={t("settings")}
                >
                  <SettingsIcon size={18} />
                </IconButton>
              </div>
            </div>

            <IconButton
              type="button"
              variant="secondary"
              className="text-[var(--color-text-secondary)] md:hidden"
              onClick={onOpenMobileMenu}
              label={t("menu")}
            >
              <MoreIcon size={18} />
            </IconButton>
          </>
        )}
      </div>
    </header>
  );
}
