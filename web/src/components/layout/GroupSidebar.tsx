import { useUIStore } from "../../stores/useUIStore";
import { groupConnectionCount } from "../../features/connect/protocol";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Trash2 } from "lucide-react";
import { instanceName } from "../../features/connect/instanceName";
import { useTranslation } from "react-i18next";
import { GroupMeta } from "../../types";
import { classNames } from "../../utils/classNames";
import type { GroupControl } from "../../utils/groupControls";
import { groupRunMenuActions } from "./groupRunMenuActions";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import {
  CloseIcon,
  FolderIcon,
  ChevronDownIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  PlusIcon,
} from "../Icons";
import { GroupSidebarItem } from "./GroupSidebarItem";
import { GroupSidebarSortableList } from "./GroupSidebarSortableList";
import { SIDEBAR_MAX_WIDTH, SIDEBAR_MIN_WIDTH } from "../../stores/useUIStore";
import { useBrandingStore } from "../../stores";
import { resolveThemeAwareLogoUrl } from "../../utils/branding";
import { Button } from "../ui/button";
import { IconButton } from "../ui/icon-button";
import { getSidebarReorderActivation, groupSidebarScrollClass } from "./groupSidebarModel";
import { CodexVoiceSidebarDock } from "../../features/codexVoice/CodexVoiceShellSurfaces";
import type { CodexVoiceShellState } from "../../features/codexVoice/useCodexVoiceShell";
import { SidebarResizeHandle } from "./SidebarResizeHandle";
import { SidebarMobileOverlay } from "./SidebarMobileOverlay";
import { ConnectSidebar } from "../../features/connect/ConnectSidebar";
import type { ConnectWorkbench } from "../../features/connect/useConnectWorkbench";

export interface GroupSidebarProps {
  connect?: ConnectWorkbench;
  orderedGroups: GroupMeta[];
  archivedGroupIds: string[];
  selectedGroupId: string;
  isOpen: boolean;
  isCollapsed: boolean;
  sidebarWidth: number;
  isDark: boolean;
  readOnly?: boolean;
  codexVoice?: CodexVoiceShellState;
  onSelectGroup: (groupId: string) => void;
  onWarmGroup?: (groupId: string) => void;
  onCreateGroup?: () => void;
  onClose: () => void;
  onToggleCollapse: () => void;
  onResizeWidth: (width: number) => void;
  onReorderSection: (section: "working" | "archived", fromIndex: number, toIndex: number) => void;
  onArchiveGroup: (groupId: string) => void;
  onRestoreGroup: (groupId: string) => void;
  onOpenGroupConnections?: (groupId: string) => void;
  /** Launch/pause/stop a group from its menu; omitted for read-only viewers. */
  onControlGroup?: (groupId: string, control: GroupControl) => void;
  /** Delete a group from its menu; the handler owns the confirmation. */
  onDeleteGroup?: (groupId: string) => void;
}

export function GroupSidebar({
  connect,
  orderedGroups,
  archivedGroupIds,
  selectedGroupId,
  isOpen,
  isCollapsed,
  sidebarWidth,
  isDark,
  readOnly,
  codexVoice,
  onSelectGroup,
  onWarmGroup,
  onCreateGroup,
  onClose,
  onToggleCollapse,
  onResizeWidth,
  onReorderSection,
  onArchiveGroup,
  onRestoreGroup,
  onOpenGroupConnections,
  onControlGroup,
  onDeleteGroup,
}: GroupSidebarProps) {
  const { t } = useTranslation("layout");
  const controlsBusy = useUIStore((state) => state.busy.startsWith("group-"));
  const branding = useBrandingStore((s) => s.branding);
  const logoSrc = resolveThemeAwareLogoUrl(branding.logo_icon_url, isDark);
  const sidebarRef = useRef<HTMLElement | null>(null);
  const dragStateRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [isResizing, setIsResizing] = useState(false);
  const archivedSet = useMemo(() => new Set(archivedGroupIds), [archivedGroupIds]);
  const workingGroups = useMemo(
    () => orderedGroups.filter((g) => !archivedSet.has(String(g.group_id || "").trim())),
    [archivedSet, orderedGroups],
  );
  const archivedGroups = useMemo(
    () => orderedGroups.filter((g) => archivedSet.has(String(g.group_id || "").trim())),
    [archivedSet, orderedGroups],
  );
  const collapsedGroups = useMemo(() => {
    if (!isCollapsed) return workingGroups;
    const selectedArchived = archivedGroups.find(
      (g) => String(g.group_id || "").trim() === String(selectedGroupId || "").trim(),
    );
    return selectedArchived ? [...workingGroups, selectedArchived] : workingGroups;
  }, [archivedGroups, isCollapsed, selectedGroupId, workingGroups]);
  const [archivedOpen, setArchivedOpen] = useState(
    () =>
      archivedGroups.some(
        (g) => String(g.group_id || "").trim() === String(selectedGroupId || "").trim(),
      ) ||
      (orderedGroups.length > 0 && workingGroups.length === 0 && archivedGroups.length > 0),
  );
  const selectedArchived = useMemo(
    () =>
      archivedGroups.some(
        (g) => String(g.group_id || "").trim() === String(selectedGroupId || "").trim(),
      ),
    [archivedGroups, selectedGroupId],
  );
  const autoArchivedOpen =
    selectedArchived ||
    (orderedGroups.length > 0 && workingGroups.length === 0 && archivedGroups.length > 0);
  const archivedPanelOpen = archivedOpen || autoArchivedOpen;

  useEffect(() => {
    if (!isResizing) return undefined;

    const handlePointerMove = (event: PointerEvent) => {
      const drag = dragStateRef.current;
      if (!drag) return;
      onResizeWidth(drag.startWidth + (event.clientX - drag.startX));
    };

    const finishResize = () => {
      dragStateRef.current = null;
      setIsResizing(false);
      document.body.style.removeProperty("cursor");
      document.body.style.removeProperty("user-select");
    };

    window.addEventListener("pointermove", handlePointerMove);
    window.addEventListener("pointerup", finishResize);
    window.addEventListener("pointercancel", finishResize);
    return () => {
      window.removeEventListener("pointermove", handlePointerMove);
      window.removeEventListener("pointerup", finishResize);
      window.removeEventListener("pointercancel", finishResize);
      finishResize();
    };
  }, [isResizing, onResizeWidth]);

  const handleResizeStart = useCallback(
    (event: React.PointerEvent<HTMLDivElement>) => {
      if (isCollapsed) return;
      event.preventDefault();
      event.stopPropagation();
      dragStateRef.current = {
        startX: event.clientX,
        startWidth: sidebarRef.current?.getBoundingClientRect().width || sidebarWidth,
      };
      setIsResizing(true);
      document.body.style.setProperty("cursor", "col-resize");
      document.body.style.setProperty("user-select", "none");
    },
    [isCollapsed, sidebarWidth],
  );

  const runActionsFor = useCallback(
    (group: GroupMeta) => {
      if (!onControlGroup || readOnly) return [];
      const gid = String(group.group_id || "");
      return groupRunMenuActions(
        getGroupStatusFromSource(group).key,
        t,
        (control) => onControlGroup(gid, control),
        controlsBusy,
      );
    },
    [onControlGroup, readOnly, t, controlsBusy],
  );

  const trailingActionsFor = useCallback(
    (group: GroupMeta) => {
      if (!onDeleteGroup || readOnly) return [];
      const gid = String(group.group_id || "");
      return [
        {
          label: t("deleteGroup"),
          disabled: controlsBusy,
          icon: <Trash2 size={15} />,
          tone: "danger" as const,
          section: "danger",
          onClick: () => onDeleteGroup(gid),
        },
      ];
    },
    [onDeleteGroup, readOnly, t, controlsBusy],
  );

  const renderGroupList = useCallback(
    (groups: GroupMeta[], section: "working" | "archived") => {
      const isArchivedSection = section === "archived";
      const menuActionLabel = isArchivedSection ? t("restoreGroup") : t("archiveGroup");
      const handleMenuAction = (gid: string) => {
        if (isArchivedSection) {
          onRestoreGroup(gid);
          return;
        }
        setArchivedOpen(true);
        onArchiveGroup(gid);
      };
      if (getSidebarReorderActivation({ isCollapsed, readOnly }) !== "disabled") {
        return (
          <GroupSidebarSortableList
            groups={groups}
            section={section}
            selectedGroupId={selectedGroupId}
            isDark={isDark}
            isCollapsed={false}
            readOnly={readOnly}
            menuActionLabel={menuActionLabel}
            connectionsLabel={t("groupConnections.title")}
            onOpenConnections={onOpenGroupConnections}
            connectionSummary={connect?.groupConnections}
            menuAriaLabel={t("groupActions")}
            reorderInstructions={t("reorderWithKeyboard")}
            onMenuAction={handleMenuAction}
            runActionsFor={runActionsFor}
            trailingActionsFor={trailingActionsFor}
            onReorderSection={onReorderSection}
            onSelectGroup={onSelectGroup}
            onWarmGroup={onWarmGroup}
            onClose={onClose}
          />
        );
      }
      return (
        <div className={classNames(isCollapsed ? "flex flex-col items-center gap-2" : "space-y-1")}>
          {groups.map((g) => {
            const gid = String(g.group_id || "");
            return (
              <GroupSidebarItem
                key={gid}
                group={g}
                isActive={gid === selectedGroupId}
                isCollapsed={isCollapsed}
                isArchived={isArchivedSection}
                connectionsLabel={t("groupConnections.title")}
                connection={groupConnectionCount(connect?.groupConnections, gid)}
                onOpenConnections={
                  onOpenGroupConnections ? () => onOpenGroupConnections(gid) : undefined
                }
                menuActionLabel={isCollapsed ? undefined : menuActionLabel}
                menuAriaLabel={isCollapsed ? undefined : `${t("groupActions")} · ${g.title || gid}`}
                onMenuAction={isCollapsed ? undefined : () => handleMenuAction(gid)}
                runActions={isCollapsed ? undefined : runActionsFor(g)}
                trailingActions={isCollapsed ? undefined : trailingActionsFor(g)}
                onSelect={() => {
                  onSelectGroup(gid);
                  if (window.matchMedia("(max-width: 767px)").matches) onClose();
                }}
                onWarm={gid === selectedGroupId ? undefined : () => onWarmGroup?.(gid)}
              />
            );
          })}
        </div>
      );
    },
    [
      isCollapsed,
      isDark,
      onArchiveGroup,
      onClose,
      onReorderSection,
      onRestoreGroup,
      onOpenGroupConnections,
      connect?.groupConnections,
      onSelectGroup,
      onWarmGroup,
      readOnly,
      runActionsFor,
      selectedGroupId,
      t,
      trailingActionsFor,
    ],
  );

  return (
    <>
      <aside
        ref={sidebarRef}
        className={classNames(
          "h-full min-h-0 flex flex-col glass-sidebar",
          "fixed inset-y-0 left-0 z-50 md:relative md:inset-auto md:z-40",
          isResizing ? "transition-none" : "transition-[width,transform] duration-300 ease-out",
          isCollapsed ? "w-[60px]" : "w-[248px] md:w-[var(--sidebar-width)]",
          isOpen ? "translate-x-0" : "-translate-x-full",
          "md:translate-x-0",
        )}
      >
        {/* Header */}
        <div className="px-3 py-2.5">
          <div
            className={classNames(
              "flex items-center gap-1.5",
              isCollapsed ? "justify-center" : "justify-between",
            )}
          >
            <div
              className={classNames("flex min-w-0 flex-1 items-center", isCollapsed ? "" : "gap-3")}
            >
              <div
                className={classNames(
                  "flex items-center justify-center overflow-hidden rounded-xl bg-transparent",
                  "w-10 h-10 shrink-0",
                  "text-[rgb(35,36,37)] dark:text-white",
                )}
              >
                <img
                  src={logoSrc}
                  alt={`${branding.product_name} logo`}
                  className={classNames("object-contain", isCollapsed ? "w-6 h-6" : "h-8 w-8")}
                />
              </div>
              {!isCollapsed && (
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[15px] font-semibold tracking-[-0.035em] text-[var(--color-text-primary)]">
                    {branding.product_name}
                  </div>
                </div>
              )}
            </div>

            {!isCollapsed && !readOnly && onCreateGroup && (
              <Button
                type="button"
                variant="secondary"
                size="sm"
                className="border-0 text-[13px] shadow-none"
                onClick={onCreateGroup}
                title={t("createNewGroup")}
                aria-label={t("createNewGroup")}
              >
                {t("newGroup")}
              </Button>
            )}

            {!isCollapsed && (
              <div className="flex shrink-0 items-center gap-2">
                {/* Collapse button - desktop only */}
                <IconButton
                  type="button"
                  variant="ghost"
                  size="sm"
                  className="hidden text-[var(--color-text-tertiary)] md:inline-flex"
                  onClick={onToggleCollapse}
                  label={t("collapseSidebar")}
                >
                  <ChevronLeftIcon size={16} />
                </IconButton>
                {/* Close button - mobile only */}
                <IconButton
                  type="button"
                  variant="secondary"
                  size="touch"
                  className="text-[var(--color-text-primary)] md:hidden"
                  onClick={onClose}
                  label={t("closeSidebar")}
                >
                  <CloseIcon size={18} />
                </IconButton>
              </div>
            )}
          </div>
        </div>

        {/* Collapsed: expand button and new button */}
        {isCollapsed && (
          <div className="p-2 flex flex-col items-center gap-2">
            <IconButton
              type="button"
              variant="secondary"
              size="touch"
              className="text-[var(--color-text-primary)]"
              onClick={onToggleCollapse}
              label={t("expandSidebar")}
            >
              <ChevronRightIcon size={18} />
            </IconButton>
            {!readOnly && onCreateGroup && (
              <IconButton
                type="button"
                size="touch"
                onClick={onCreateGroup}
                label={t("createNewGroup")}
              >
                <PlusIcon size={18} />
              </IconButton>
            )}
          </div>
        )}

        {/* Group list */}
        <div className={groupSidebarScrollClass(isCollapsed)}>
          {!isCollapsed && (
            <div
              className="flex min-w-0 items-baseline gap-2 px-2 pb-2"
              title={
                connect?.ownInstance
                  ? [connect.ownInstance.display_name, connect.ownInstance.public_origin]
                      .filter(Boolean)
                      .join(" · ")
                  : undefined
              }
            >
              <div className="shrink-0 text-xs font-semibold uppercase tracking-[0.18em] text-[var(--color-text-tertiary)]">
                {t("workingGroups")}
              </div>
              {connect?.ownInstance && (
                <span className="min-w-0 flex-1 truncate text-right text-xs text-[var(--color-text-secondary)]">
                  {instanceName(connect.ownInstance, connect.instances)}
                </span>
              )}
            </div>
          )}

          {renderGroupList(isCollapsed ? collapsedGroups : workingGroups, "working")}

          {!isCollapsed && archivedGroups.length > 0 && (
            <div className="mt-4">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                className="w-full justify-between px-2 text-[var(--color-text-primary)]"
                onClick={() => setArchivedOpen((prev) => !prev)}
                aria-expanded={archivedPanelOpen}
              >
                <div className="flex items-center gap-2 min-w-0">
                  <span className="text-xs font-semibold uppercase tracking-[0.15em] text-[var(--color-text-tertiary)]">
                    {t("archivedGroups")}
                  </span>
                  <span className="px-2 py-0.5 rounded-full text-xs font-semibold bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]">
                    {archivedGroups.length}
                  </span>
                </div>
                <ChevronDownIcon
                  size={16}
                  className={classNames(
                    "transition-transform",
                    archivedPanelOpen ? "rotate-180" : "",
                  )}
                />
              </Button>
              {archivedPanelOpen && (
                <div className="mt-2">{renderGroupList(archivedGroups, "archived")}</div>
              )}
            </div>
          )}

          {/* Empty state */}
          {!orderedGroups.length && !isCollapsed && (
            <div className="p-6 text-center">
              <div
                className={classNames(
                  "w-16 h-16 mx-auto mb-4 rounded-2xl flex items-center justify-center glass-card",
                  "text-[var(--color-text-tertiary)]",
                )}
              >
                <FolderIcon size={32} />
              </div>
              <div className="text-sm mb-2 font-medium text-[var(--color-text-secondary)]">
                {t("noGroupsYet")}
              </div>
              <div className="text-xs mb-5 max-w-[200px] mx-auto leading-relaxed text-[var(--color-text-tertiary)]">
                {t("noGroupsDescription")}
              </div>
              {!readOnly && onCreateGroup && (
                <Button type="button" onClick={onCreateGroup}>
                  {t("createFirstGroup")}
                </Button>
              )}
            </div>
          )}
          {connect ? (
            <ConnectSidebar
              workbench={connect}
              collapsed={isCollapsed}
              onSelected={() => {
                if (window.matchMedia("(max-width: 767px)").matches) onClose();
              }}
            />
          ) : null}
        </div>

        {!readOnly && codexVoice ? (
          <CodexVoiceSidebarDock voice={codexVoice} collapsed={isCollapsed} />
        ) : null}

        {!isCollapsed && (
          <SidebarResizeHandle
            width={sidebarWidth}
            min={SIDEBAR_MIN_WIDTH}
            max={SIDEBAR_MAX_WIDTH}
            resizing={isResizing}
            label={t("resizeSidebar")}
            onPointerDown={handleResizeStart}
          />
        )}
      </aside>

      <SidebarMobileOverlay open={isOpen} onClose={onClose} />
    </>
  );
}
