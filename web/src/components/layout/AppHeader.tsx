import { useEffect, useState, type Ref } from "react";
import { LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Actor, GroupDoc, GroupRuntimeStatus, TextScale, Theme } from "../../types";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { getLaunchControlMode, resolveGroupControls } from "../../utils/groupControls";
import { classNames } from "../../utils/classNames";
import {
  ClipboardIcon,
  EditIcon,
  SearchIcon,
  PlayIcon,
  PauseIcon,
  StopIcon,
  MoreIcon,
  MenuIcon,
} from "../Icons";
import { IconButton } from "../ui/icon-button";
import { GroupStatusIndicator } from "./GroupStatusIndicator";
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
  accountLabel,
  onOpenAccount,
  onOpenMobileMenu,
  sseStatus,
  workControlsRef,
  sidePanelControlsRef,
}: AppHeaderProps) {
  const { t } = useTranslation("layout");
  const [pendingToggleAction, setPendingToggleAction] = useState<"launch" | "pause" | null>(null);
  const [hasObservedGroupBusy, setHasObservedGroupBusy] = useState(false);
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
  const selectedStatusKey = selectedStatus?.key ?? null;
  const launchMode = getLaunchControlMode(selectedStatusKey);
  const { launchDisabled, pauseDisabled, stopDisabled } = resolveGroupControls({
    selectedGroupId,
    actorCount: actors.length,
    statusKey: selectedStatusKey,
    busy,
  });
  const isPauseAction = selectedStatusKey === "run";
  const toggleDisabled =
    (isPauseAction ? pauseDisabled : launchDisabled) || pendingToggleAction !== null;
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
        <h1
          className="min-w-0 truncate text-base font-semibold leading-tight text-[var(--color-text-primary)] md:text-[1.125rem]"
          title={groupTitle}
        >
          {groupTitle}
        </h1>
        {canEditGroup && (
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            className="shrink-0 text-[var(--color-text-tertiary)]"
            label={t("editGroup")}
            aria-haspopup="dialog"
            data-group-title-edit
            onClick={onOpenGroupEdit}
          >
            <EditIcon size={16} />
          </IconButton>
        )}
        {selectedGroupId && sseStatus !== "connected" && (
          <span
            className={classNames(
              "h-2 w-2 shrink-0 rounded-full",
              sseStatus === "connecting" ? "bg-amber-400 animate-pulse" : "bg-rose-500",
            )}
            title={sseStatus === "connecting" ? t("reconnecting") : t("disconnected")}
          />
        )}
        {selectedStatus && (
          <span className="hidden shrink-0 @min-[480px]/group-header:inline-flex">
            <GroupStatusIndicator status={selectedStatus} />
          </span>
        )}
      </div>

      {!webReadOnly && (
        <div
          className="hidden shrink-0 items-center gap-0.5 @min-[760px]/group-header:flex"
          data-group-run-controls
        >
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleToggleClick}
            disabled={toggleDisabled}
            aria-busy={pendingToggleAction !== null || isGroupBusy}
            className="text-[var(--color-text-secondary)]"
            label={toggleTitle}
          >
            {pendingToggleAction !== null || (isGroupBusy && busy !== "group-stop") ? (
              <LoaderCircle size={17} className="animate-spin" />
            ) : isPauseAction ? (
              <PauseIcon size={17} />
            ) : (
              <PlayIcon size={17} />
            )}
          </IconButton>
          <IconButton
            type="button"
            variant="ghost"
            size="sm"
            onClick={handleStopClick}
            disabled={stopDisabled || selectedStatusKey === "stop"}
            aria-busy={busy === "group-stop"}
            className="text-[var(--color-text-secondary)]"
            label={t("stopAllAgents")}
          >
            {busy === "group-stop" ? (
              <LoaderCircle size={17} className="animate-spin" />
            ) : (
              <StopIcon size={17} />
            )}
          </IconButton>
        </div>
      )}

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
