import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Actor, GroupDoc, GroupRuntimeStatus, TextScale, Theme } from "../../types";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { getGroupControlVisual, getLaunchControlMode, resolveGroupControls } from "../../utils/groupControls";
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
  EditIcon,
  MoreIcon,
  MenuIcon
} from "../Icons";

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
  onOpenMobileMenu,
  sseStatus,
}: AppHeaderProps) {
  const { t } = useTranslation('layout');
  const [pendingToggleAction, setPendingToggleAction] = useState<"launch" | "pause" | null>(null);
  const [hasObservedGroupBusy, setHasObservedGroupBusy] = useState(false);
  const headerIconButtonBaseClass =
    "flex items-center justify-center h-10 w-10 rounded-[14px] transition-all shrink-0";
  const headerRailClass =
    "flex items-center gap-1 p-[3px]";
  const headerUtilityRailClass =
    "flex items-center gap-0.5 p-[3px]";
  const headerMinorActionClass =
    "hidden md:inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg border border-transparent bg-transparent text-[var(--color-text-tertiary)] transition-all hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]";
  const headerRailButtonClass =
    "flex items-center justify-center h-9 w-9 rounded-[14px] transition-all shrink-0 border border-transparent bg-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-45 disabled:text-[var(--color-text-tertiary)] disabled:hover:bg-transparent disabled:hover:text-[var(--color-text-tertiary)]";
  const headerUtilityButtonClass =
    "flex items-center justify-center h-8 w-8 rounded-xl transition-all shrink-0 border border-transparent bg-transparent text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]";
  const headerRailDividerClass = "mx-1 h-5 w-px bg-[var(--glass-border-subtle)]";
  const selectedStatus = selectedGroupId ? getGroupStatusFromSource({
    running: selectedGroupRunning,
    state: (selectedGroupRuntimeStatus?.lifecycle_state as GroupDoc["state"] | undefined) || groupDoc?.state,
    runtime_status: selectedGroupRuntimeStatus || undefined,
  }) : null;
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
  const toggleDisabled = (isPauseAction ? pauseDisabled : launchDisabled) || pendingToggleAction !== null;
  const toggleHardUnavailable = isPauseAction ? pauseHardUnavailable : launchHardUnavailable;
  const toggleTitle = isPauseAction
    ? t('pauseDelivery')
    : launchMode === "activate"
      ? t('resumeDelivery')
      : t('launchAllAgents');
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
    const launchSettled = pendingToggleAction === "launch" && (selectedStatusKey === "run" || selectedStatusKey === "idle");
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
    <header
      className="absolute inset-x-0 top-0 z-20 flex h-14 flex-shrink-0 items-center justify-between gap-3 px-4 glass-header md:relative md:inset-auto md:px-5"
    >
      <div className="flex min-w-0 items-center gap-2">
        <button
          className={classNames(
            "md:hidden -ml-1",
            headerIconButtonBaseClass,
            "glass-btn",
            "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
          )}
          onClick={onOpenSidebar}
          aria-label={t('openSidebar')}
        >
          <MenuIcon size={18} />
        </button>

        <div className="min-w-0 flex items-center gap-2">
          <div className="flex min-w-0 items-center gap-1.5">
            <h1 className="truncate text-base font-semibold leading-tight text-[var(--color-text-primary)] md:text-[1.125rem]">
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : t('selectGroup'))}
            </h1>
            {selectedGroupId && sseStatus !== "connected" && (
              <span
                className={classNames(
                  "h-2 w-2 flex-shrink-0 rounded-full",
                  sseStatus === "connecting" ? "bg-amber-400 animate-pulse" : "bg-rose-500"
                )}
                title={sseStatus === "connecting" ? t('reconnecting') : t('disconnected')}
              />
            )}
            {selectedStatus && (
              <span
                className={classNames(
                  "h-2.5 w-2.5 flex-shrink-0 rounded-full",
                  selectedStatus.dotClass
                )}
                title={selectedStatus.label}
              />
            )}
          </div>

          {selectedGroupId && !webReadOnly && onOpenGroupEdit && (
            <button
              className={headerMinorActionClass}
              onClick={onOpenGroupEdit}
              title={t('editGroup')}
              aria-label={t('editGroup')}
            >
              <EditIcon size={14} />
            </button>
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
                <button
                  onClick={onOpenSearch}
                  disabled={!selectedGroupId}
                  className={headerRailButtonClass}
                  title={t('searchMessages')}
                >
                  <span className="sr-only">{t('searchMessages')}</span>
                  <SearchIcon size={17} />
                </button>

                <button
                  onClick={onOpenContext}
                  disabled={!selectedGroupId}
                  className={headerRailButtonClass}
                  title={t('context')}
                >
                  <span className="sr-only">{t('context')}</span>
                  <ClipboardIcon size={17} />
                </button>
                <span className={headerRailDividerClass} aria-hidden="true" />
                <button
                  onClick={handleToggleClick}
                  disabled={toggleDisabled}
                  className={classNames(
                    "flex items-center justify-center w-10 h-10 rounded-xl transition-all shrink-0",
                    toggleControl.className,
                    toggleHardUnavailable && "opacity-45"
                  )}
                  title={toggleTitle}
                  aria-pressed={toggleControl.active}
                >
                  <span className="sr-only">{toggleTitle}</span>
                  {isPauseAction ? <PauseIcon size={17} /> : <PlayIcon size={17} />}
                </button>

                <button
                  onClick={handleStopClick}
                  disabled={stopDisabled}
                  className={classNames(
                    "flex items-center justify-center w-10 h-10 rounded-xl transition-all shrink-0",
                    stopControl.className,
                    stopHardUnavailable && "opacity-45"
                  )}
                  title={t('stopAllAgents')}
                  aria-pressed={stopControl.active}
                >
                  <span className="sr-only">{t('stopAllAgents')}</span>
                  <StopIcon size={17} />
                </button>
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
                  className={classNames(headerUtilityButtonClass, "text-[10px] font-semibold tracking-[0.04em]")}
                />
                <span className="mx-0.5 h-4 w-px bg-[var(--glass-border-subtle)]" aria-hidden="true" />
                <button
                  onClick={onOpenSettings}
                  disabled={!selectedGroupId}
                  className={classNames(headerUtilityButtonClass, "disabled:opacity-45 disabled:text-[var(--color-text-tertiary)]")}
                  title={t('settings')}
                >
                  <span className="sr-only">{t('settings')}</span>
                  <SettingsIcon size={18} />
                </button>
              </div>
            </div>

            <button
              className={classNames(
                "md:hidden",
                headerIconButtonBaseClass,
                "glass-btn",
                "text-[var(--color-text-secondary)] hover:text-[var(--color-text-primary)]"
              )}
              onClick={onOpenMobileMenu}
              title={t('menu')}
            >
              <MoreIcon size={18} />
            </button>
          </>
        )}
      </div>

    </header>
  );
}
