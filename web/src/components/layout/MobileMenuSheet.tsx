import { useEffect, useState } from "react";
import "./MobileMenuSheet.css";
import { useTranslation } from "react-i18next";
import { Actor, GroupDoc, TextScale, Theme } from "../../types";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import {
  getGroupControlVisual,
  getLaunchControlMode,
  resolveGroupControls,
} from "../../utils/groupControls";
import { classNames } from "../../utils/classNames";
import { useModalA11y } from "../../hooks/useModalA11y";
import { LanguageSwitcher } from "../LanguageSwitcher";
import { TextScaleSwitcher } from "../TextScaleSwitcher";
import {
  SearchIcon,
  ClipboardIcon,
  SettingsIcon,
  AccountIcon,
  MonitorIcon,
  SunIcon,
  MoonIcon,
  EditIcon,
  PlayIcon,
  StopIcon,
  PauseIcon,
  CloseIcon,
} from "../Icons";
import { GroupStatusIndicator } from "./GroupStatusIndicator";

export interface MobileMenuSheetProps {
  isOpen: boolean;
  isDark: boolean;
  theme: Theme;
  textScale: TextScale;
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  selectedGroupRunning: boolean;
  actors: Actor[];
  busy: string;
  onClose: () => void;
  onThemeChange: (theme: Theme) => void;
  onTextScaleChange: (scale: TextScale) => void;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  onOpenSettings: () => void;
  canAccessAccount: boolean;
  onOpenAccount: () => void;
  onOpenGroupEdit?: () => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSetGroupState: (state: "active" | "paused" | "idle") => void | Promise<void>;
}

export function MobileMenuSheet({
  isOpen,
  isDark,
  theme,
  textScale,
  selectedGroupId,
  groupDoc,
  selectedGroupRunning,
  actors,
  busy,
  onClose,
  onThemeChange,
  onTextScaleChange,
  onOpenSearch,
  onOpenContext,
  onOpenSettings,
  canAccessAccount,
  onOpenAccount,
  onOpenGroupEdit,
  onStartGroup,
  onStopGroup,
  onSetGroupState,
}: MobileMenuSheetProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  const { t } = useTranslation("layout");
  const [pendingToggleAction, setPendingToggleAction] = useState<"launch" | "pause" | null>(null);
  const [hasObservedGroupBusy, setHasObservedGroupBusy] = useState(false);
  const selectedStatus = selectedGroupId
    ? getGroupStatusFromSource({
        running: selectedGroupRunning,
        state: groupDoc?.state,
        runtime_status: groupDoc?.runtime_status,
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
  const toggleLabel = isPauseAction ? t("pauseState") : t("runState");
  const isGroupBusy = busy.startsWith("group-");
  const themeLabel =
    theme === "system" ? t("themeSystem") : theme === "dark" ? t("themeDark") : t("themeLight");
  const ThemeIcon = theme === "system" ? MonitorIcon : theme === "dark" ? MoonIcon : SunIcon;
  const nextTheme: Theme = theme === "light" ? "dark" : theme === "dark" ? "system" : "light";
  const runtimeHint =
    selectedStatusKey === "paused"
      ? t("runtimeHintPaused")
      : selectedStatusKey === "stop"
        ? t("runtimeHintStop")
        : selectedStatusKey === "idle"
          ? t("runtimeHintIdle")
          : t("runtimeHintRun");
  const sectionCardClass =
    "rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-2 shadow-sm backdrop-blur-xl";
  const sectionTitleClass =
    "px-2.5 pb-1 text-[11px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]";
  const rowButtonClass =
    "w-full flex items-center justify-between gap-3 rounded-xl px-3.5 py-3 text-sm transition-all text-[var(--color-text-primary)] hover:bg-black/5 disabled:opacity-45 dark:hover:bg-white/6";

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
    onClose();
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
    onClose();
    void onSetGroupState("paused");
  };

  const handleStopClick = () => {
    if (stopDisabled || selectedStatusKey === "stop") return;
    onClose();
    onStopGroup();
  };

  const handleToggleClick = () => {
    if (isPauseAction) {
      handlePauseClick();
      return;
    }
    handleLaunchClick();
  };
  if (!isOpen) return null;

  return (
    <div className="mobile-menu-viewport fixed inset-0 z-50 md:hidden animate-fade-in">
      <div
        className="absolute inset-0 glass-overlay"
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        aria-hidden="true"
      />

      <div
        ref={modalRef}
        className="mobile-menu-panel rounded-t-3xl glass-modal animate-slide-up transform transition-transform"
        role="dialog"
        aria-modal="true"
        aria-label={t("menu")}
      >
        <div className="flex justify-center pt-3 pb-1" onClick={onClose}>
          <div className="w-12 h-1.5 rounded-full bg-black/15 dark:bg-white/20" />
        </div>

        <div className="px-6 pb-4 flex items-center justify-between gap-3">
          <div className="min-w-0">
            <div
              className={classNames(
                "text-lg font-bold truncate",
                "text-[var(--color-text-primary)]",
              )}
            >
              {groupDoc?.title || (selectedGroupId ? selectedGroupId : t("menu"))}
            </div>
            {selectedStatus && (
              <div className="flex items-center gap-2 mt-1">
                <GroupStatusIndicator status={selectedStatus} variant="badge" />
              </div>
            )}
          </div>
          <button
            onClick={onClose}
            className={classNames(
              "p-2 rounded-full transition-colors glass-btn",
              "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]",
            )}
            aria-label={t("closeMenu")}
          >
            <CloseIcon size={20} />
          </button>
        </div>

        <div className="mobile-menu-content p-4 space-y-4">
          {!selectedGroupId && (
            <div className={classNames("text-sm px-1 pb-2", "text-[var(--color-text-tertiary)]")}>
              {t("selectGroupToEnable")}
            </div>
          )}

          <button
            className={classNames(
              "w-full flex items-center justify-between gap-3 px-4 py-3.5 rounded-2xl text-sm font-medium transition-all min-h-[52px] disabled:opacity-50 glass-btn shadow-sm",
              "text-[var(--color-text-primary)]",
            )}
            onClick={() => {
              onClose();
              onOpenSearch();
            }}
            disabled={!selectedGroupId}
          >
            <div className="flex items-center gap-3">
              <SearchIcon size={18} />
              <span>{t("searchMessagesButton")}</span>
            </div>
          </button>

          <section className={sectionCardClass}>
            <div className={sectionTitleClass}>{t("workspaceSection")}</div>
            {canAccessAccount ? (
              <button
                className={rowButtonClass}
                onClick={() => {
                  onClose();
                  onOpenAccount();
                }}
              >
                <div className="flex items-center gap-3">
                  <AccountIcon size={18} />
                  <span>{t("account")}</span>
                </div>
              </button>
            ) : null}
            <button
              className={rowButtonClass}
              onClick={() => {
                onClose();
                onOpenContext();
              }}
              disabled={!selectedGroupId}
            >
              <div className="flex items-center gap-3">
                <ClipboardIcon size={18} />
                <span>{t("contextButton")}</span>
              </div>
            </button>
            <button
              className={rowButtonClass}
              onClick={() => {
                onClose();
                onOpenSettings();
              }}
              disabled={!selectedGroupId && !canAccessAccount}
            >
              <div className="flex items-center gap-3">
                <SettingsIcon size={18} />
                <span>{t("settingsButton")}</span>
              </div>
            </button>
            {onOpenGroupEdit ? (
              <button
                className={rowButtonClass}
                onClick={() => {
                  onClose();
                  onOpenGroupEdit();
                }}
                disabled={!selectedGroupId}
              >
                <div className="flex items-center gap-3">
                  <EditIcon size={18} />
                  <span>{t("editGroupDetails")}</span>
                </div>
              </button>
            ) : null}
          </section>

          <section className={sectionCardClass}>
            <div className={sectionTitleClass}>{t("appearanceSection")}</div>
            <button className={rowButtonClass} onClick={() => onThemeChange(nextTheme)}>
              <div className="flex items-center gap-3">
                <ThemeIcon size={18} />
                <span>{t("themeLabel")}</span>
              </div>
              <span className="text-[13px] font-medium text-[var(--color-text-tertiary)]">
                {themeLabel}
              </span>
            </button>
            <TextScaleSwitcher
              textScale={textScale}
              onTextScaleChange={onTextScaleChange}
              variant="row"
            />
            <LanguageSwitcher isDark={isDark} variant="row" />
          </section>

          <section className={sectionCardClass}>
            <div className={sectionTitleClass}>{t("runtimeSection")}</div>
            <div className="px-2.5 pb-1 text-[12px] leading-5 text-[var(--color-text-tertiary)]">
              {runtimeHint}
            </div>
            <div className="mt-2 flex items-center gap-1 rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--glass-bg)] p-1">
              <button
                className={classNames(
                  "flex-1 flex items-center justify-center gap-2 rounded-xl px-3 py-3 text-sm font-medium transition-all min-h-[48px]",
                  toggleControl.className,
                  toggleHardUnavailable && "opacity-45",
                )}
                onClick={handleToggleClick}
                disabled={toggleDisabled}
                aria-pressed={toggleControl.active}
              >
                {isPauseAction ? <PauseIcon size={18} /> : <PlayIcon size={18} />}
                <span>{toggleLabel}</span>
              </button>
              <button
                className={classNames(
                  "flex-1 flex items-center justify-center gap-2 rounded-xl px-3 py-3 text-sm font-medium transition-all min-h-[48px]",
                  stopControl.className,
                  stopHardUnavailable && "opacity-45",
                )}
                onClick={handleStopClick}
                disabled={stopDisabled}
                aria-pressed={stopControl.active}
              >
                <StopIcon size={18} />
                <span>{t("stopState")}</span>
              </button>
            </div>
          </section>
        </div>
      </div>
    </div>
  );
}
