import { useEffect, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { ErrorBoundary } from "../ErrorBoundary";
import { AppHeader } from "../layout/AppHeader";
import { GroupSidebar } from "../layout/GroupSidebar";
import { RuntimeInspectorModal } from "../modals/RuntimeInspectorModal";
import {
  CodexVoiceMobileDock,
  CodexVoiceOverlays,
} from "../../features/codexVoice/CodexVoiceShellSurfaces";
import { useCodexVoiceShell } from "../../features/codexVoice/useCodexVoiceShell";
import { ActorTab } from "../../pages/ActorTab";
import { ChatTab } from "../../pages/chat";
import type {
  Actor,
  GroupContext,
  GroupDoc,
  GroupMeta,
  GroupRuntimeStatus,
  TextScale,
} from "../../types";
import { getSidebarWidthCssValue, SIDEBAR_COLLAPSED_WIDTH } from "../../stores/useUIStore";
import { resolveRuntimeInspectorActor } from "./appShellRuntimeActors";
import type { ComposerMentionKind } from "../../pages/chat/chatMentionSuggestions";
type AppShellProps = {
  orderedGroups: GroupMeta[];
  archivedGroupIds: string[];
  selectedGroupId: string;
  groupDoc: GroupDoc | null;
  groupContext: GroupContext | null;
  actors: Actor[];
  runtimeActors: Actor[];
  recipientActors: Actor[];
  recipientActorsBusy: boolean;
  destGroupScopeLabel: string;
  renderedActorIds: string[];
  activeTab: string;
  busy: string;
  isTransitioning: boolean;
  sidebarOpen: boolean;
  sidebarCollapsed: boolean;
  sidebarWidth: number;
  isDark: boolean;
  isSmallScreen: boolean;
  webReadOnly: boolean;
  selectedGroupRunning: boolean;
  selectedGroupRuntimeStatus: GroupRuntimeStatus | null;
  selectedGroupActorsHydrating: boolean;
  selectedGroupActorStatusProvisional: boolean;
  theme: "light" | "dark" | "system";
  textScale: TextScale;
  sseStatus: "connected" | "connecting" | "disconnected";
  groupLabelById: Record<string, string>;
  mentionFilter?: string;
  mentionKind?: ComposerMentionKind;
  mentionActorScope?: "selected" | "destination";
  mentionSelectedIndex: number;
  showMentionMenu: boolean;
  composerRef: React.RefObject<HTMLTextAreaElement | null>;
  fileInputRef: React.RefObject<HTMLInputElement | null>;
  eventContainerRef: React.MutableRefObject<HTMLDivElement | null>;
  contentRef: React.MutableRefObject<HTMLDivElement | null>;
  chatAtBottomRef: React.MutableRefObject<boolean>;
  onThemeChange: (theme: "light" | "dark" | "system") => void;
  onTextScaleChange: (scale: TextScale) => void;
  onSelectGroup: (groupId: string) => void;
  onWarmGroup: (groupId: string) => void;
  onCreateGroup: (() => void) | undefined;
  onCloseSidebar: () => void;
  onToggleSidebar: () => void;
  onResizeSidebar: (width: number) => void;
  onReorderGroupsInSection: (
    section: "working" | "archived",
    fromIndex: number,
    toIndex: number,
  ) => void;
  onArchiveGroup: (groupId: string) => void;
  onRestoreGroup: (groupId: string) => void;
  onOpenSidebar: () => void;
  onOpenGroupEdit: (() => void) | undefined;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  onStartGroup: () => void;
  onStopGroup: () => void;
  onSetGroupState: (state: "active" | "idle" | "paused") => void;
  onOpenSettings: () => void;
  canAccessAccount: boolean;
  onOpenAccount: () => void;
  onOpenMobileMenu: () => void;
  onTabChange: (tab: string) => void;
  appendComposerFiles: (files: File[]) => void;
  setMentionFilter: React.Dispatch<React.SetStateAction<string>>;
  setMentionKind: React.Dispatch<React.SetStateAction<ComposerMentionKind>>;
  setMentionActorScope: React.Dispatch<React.SetStateAction<"selected" | "destination">>;
  setMentionTargetGroupId: React.Dispatch<React.SetStateAction<string>>;
  setMentionSelectedIndex: React.Dispatch<React.SetStateAction<number>>;
  setShowMentionMenu: React.Dispatch<React.SetStateAction<boolean>>;
  getTermEpoch: (actorId: string) => number;
  onToggleActorEnabled: (actor: Actor, isRunning?: boolean) => void;
  onRelaunchActor: (actor: Actor) => void;
  onNewActorSession: (actor: Actor) => void;
  onEditActor: (actor: Actor) => void;
  onRemoveActor: (actor: Actor, activeTab: string) => void;
  onOpenActorInbox: (actor: Actor) => void;
  onRefreshActors: () => void;
  onTouchStart: (event: React.TouchEvent) => void;
  onTouchEnd: (event: React.TouchEvent) => void;
};

type MountedRuntimeActorSnapshot = { groupId: string | null; actorsById: Record<string, Actor> };

function areMountedRuntimeActorSnapshotsEqual(
  left: MountedRuntimeActorSnapshot,
  right: MountedRuntimeActorSnapshot,
): boolean {
  if (left.groupId !== right.groupId) return false;
  const leftIds = Object.keys(left.actorsById);
  const rightIds = Object.keys(right.actorsById);
  if (leftIds.length !== rightIds.length) return false;
  return leftIds.every((actorId) => left.actorsById[actorId] === right.actorsById[actorId]);
}

export function AppShell({
  orderedGroups,
  archivedGroupIds,
  selectedGroupId,
  groupDoc,
  groupContext,
  actors,
  runtimeActors,
  recipientActors,
  recipientActorsBusy,
  destGroupScopeLabel,
  renderedActorIds,
  activeTab,
  busy,
  isTransitioning,
  sidebarOpen,
  sidebarCollapsed,
  sidebarWidth,
  isDark,
  isSmallScreen,
  webReadOnly,
  selectedGroupRunning,
  selectedGroupRuntimeStatus,
  selectedGroupActorsHydrating,
  selectedGroupActorStatusProvisional,
  theme,
  textScale,
  sseStatus,
  groupLabelById,
  mentionFilter,
  mentionKind,
  mentionActorScope,
  mentionSelectedIndex,
  showMentionMenu,
  composerRef,
  fileInputRef,
  eventContainerRef,
  contentRef,
  chatAtBottomRef,
  onThemeChange,
  onTextScaleChange,
  onSelectGroup,
  onWarmGroup,
  onCreateGroup,
  onCloseSidebar,
  onToggleSidebar,
  onResizeSidebar,
  onReorderGroupsInSection,
  onArchiveGroup,
  onRestoreGroup,
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
  onTabChange,
  appendComposerFiles,
  setMentionFilter,
  setMentionKind,
  setMentionActorScope,
  setMentionTargetGroupId,
  setMentionSelectedIndex,
  setShowMentionMenu,
  getTermEpoch,
  onToggleActorEnabled,
  onRelaunchActor,
  onNewActorSession,
  onEditActor,
  onRemoveActor,
  onOpenActorInbox,
  onRefreshActors,
  onTouchStart,
  onTouchEnd,
}: AppShellProps) {
  const { t } = useTranslation("chat");
  const shellStyle = {
    "--sidebar-width": sidebarCollapsed
      ? `${SIDEBAR_COLLAPSED_WIDTH}px`
      : getSidebarWidthCssValue(sidebarWidth),
  } as CSSProperties;
  const [mountedRuntimeActorsSnapshot, setMountedRuntimeActorsSnapshot] =
    useState<MountedRuntimeActorSnapshot>({ groupId: null, actorsById: {} });
  const codexVoice = useCodexVoiceShell(!webReadOnly);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setMountedRuntimeActorsSnapshot((current) => {
        const nextActorsById = current.groupId === selectedGroupId ? { ...current.actorsById } : {};
        for (const actor of runtimeActors) {
          const actorId = String(actor.id || "").trim();
          if (actorId) nextActorsById[actorId] = actor;
        }
        const nextSnapshot = { groupId: selectedGroupId || null, actorsById: nextActorsById };
        return areMountedRuntimeActorSnapshotsEqual(current, nextSnapshot) ? current : nextSnapshot;
      });
    }, 0);
    return () => window.clearTimeout(timer);
  }, [runtimeActors, selectedGroupId]);

  return (
    <div
      className="relative h-full min-h-0 transition-[grid-template-columns] duration-300 ease-out md:grid md:[grid-template-columns:var(--sidebar-width)_minmax(0,1fr)]"
      style={shellStyle}
    >
      <GroupSidebar
        orderedGroups={orderedGroups}
        archivedGroupIds={archivedGroupIds}
        selectedGroupId={selectedGroupId}
        isOpen={sidebarOpen}
        isCollapsed={sidebarCollapsed}
        sidebarWidth={sidebarWidth}
        isDark={isDark}
        readOnly={webReadOnly}
        codexVoice={codexVoice}
        onSelectGroup={onSelectGroup}
        onWarmGroup={onWarmGroup}
        onCreateGroup={onCreateGroup}
        onClose={onCloseSidebar}
        onToggleCollapse={onToggleSidebar}
        onResizeWidth={onResizeSidebar}
        onReorderSection={onReorderGroupsInSection}
        onArchiveGroup={onArchiveGroup}
        onRestoreGroup={onRestoreGroup}
      />

      <main className="absolute inset-0 flex h-full min-h-0 flex-col overflow-hidden md:relative md:inset-auto bg-transparent md:bg-[var(--color-chat-bg)]">
        <AppHeader
          isDark={isDark}
          theme={theme}
          textScale={textScale}
          onThemeChange={onThemeChange}
          onTextScaleChange={onTextScaleChange}
          webReadOnly={webReadOnly}
          selectedGroupId={selectedGroupId}
          groupDoc={groupDoc}
          selectedGroupRunning={selectedGroupRunning}
          selectedGroupRuntimeStatus={selectedGroupRuntimeStatus}
          actors={actors}
          sseStatus={sseStatus}
          busy={busy}
          onOpenSidebar={onOpenSidebar}
          onOpenGroupEdit={onOpenGroupEdit}
          onOpenSearch={onOpenSearch}
          onOpenContext={onOpenContext}
          onStartGroup={onStartGroup}
          onStopGroup={onStopGroup}
          onSetGroupState={onSetGroupState}
          onOpenSettings={onOpenSettings}
          canAccessAccount={canAccessAccount}
          onOpenAccount={onOpenAccount}
          onOpenMobileMenu={onOpenMobileMenu}
        />

        {!webReadOnly ? <CodexVoiceMobileDock voice={codexVoice} /> : null}

        <div
          ref={contentRef}
          className={`relative flex min-h-0 flex-1 flex-col overflow-hidden transition-opacity duration-150 ${
            isTransitioning ? "opacity-0" : "opacity-100"
          }`}
          onTouchStart={onTouchStart}
          onTouchEnd={onTouchEnd}
        >
          <div className="absolute inset-0 flex min-h-0 flex-col">
            <ErrorBoundary>
              <ChatTab
                isDark={isDark}
                isSmallScreen={isSmallScreen}
                readOnly={webReadOnly}
                mobileAppHeaderReserved={!webReadOnly && codexVoice.controller.isEngaged}
                selectedGroupId={selectedGroupId}
                selectedGroupRunning={selectedGroupRunning}
                selectedGroupActorsHydrating={selectedGroupActorsHydrating}
                selectedGroupActorStatusProvisional={selectedGroupActorStatusProvisional}
                groupLabelById={groupLabelById}
                actors={actors}
                runtimeActors={runtimeActors}
                activeRuntimeActorId={activeTab !== "chat" ? activeTab : undefined}
                recipientActors={recipientActors}
                recipientActorsBusy={recipientActorsBusy}
                destGroupScopeLabel={destGroupScopeLabel}
                mentionFilter={mentionFilter}
                mentionKind={mentionKind}
                mentionActorScope={mentionActorScope}
                scrollRef={eventContainerRef}
                composerRef={composerRef}
                fileInputRef={fileInputRef}
                chatAtBottomRef={chatAtBottomRef}
                appendComposerFiles={appendComposerFiles}
                onStartGroup={onStartGroup}
                onOpenRuntimeActor={onTabChange}
                showMentionMenu={showMentionMenu}
                setShowMentionMenu={setShowMentionMenu}
                mentionSelectedIndex={mentionSelectedIndex}
                setMentionSelectedIndex={setMentionSelectedIndex}
                setMentionFilter={setMentionFilter}
                setMentionKind={setMentionKind}
                setMentionActorScope={setMentionActorScope}
                setMentionTargetGroupId={setMentionTargetGroupId}
              />
            </ErrorBoundary>
          </div>

          {renderedActorIds.map((actorId) => {
            const mountedRuntimeActors =
              mountedRuntimeActorsSnapshot.groupId === selectedGroupId
                ? mountedRuntimeActorsSnapshot.actorsById
                : {};
            const actor = resolveRuntimeInspectorActor(
              actorId,
              runtimeActors,
              mountedRuntimeActors,
            );
            const isVisible = activeTab === actorId && activeTab !== "chat";
            const agentState =
              (groupContext?.agent_states || []).find((item) => item.id === (actor?.id || "")) ||
              null;

            return (
              <RuntimeInspectorModal
                key={actorId}
                isOpen={isVisible}
                isDark={isDark}
                onClose={() => onTabChange("chat")}
                titleId={`runtime-inspector-${actorId}`}
                closeAriaLabel={t("runtimeInspectorClose", {
                  defaultValue: "Close runtime inspector",
                })}
              >
                <div className="min-h-0 flex-1 overflow-hidden">
                  <ErrorBoundary>
                    <ActorTab
                      actor={actor}
                      groupId={selectedGroupId}
                      agentState={agentState}
                      termEpoch={actor ? getTermEpoch(actor.id) : 0}
                      busy={busy}
                      isDark={isDark}
                      isSmallScreen={isSmallScreen}
                      isVisible={isVisible}
                      readOnly={webReadOnly}
                      actorStatusProvisional={selectedGroupActorStatusProvisional}
                      onToggleEnabled={(isRunning) =>
                        actor && onToggleActorEnabled(actor, isRunning)
                      }
                      onRelaunch={() => actor && onRelaunchActor(actor)}
                      onNewSession={() => actor && onNewActorSession(actor)}
                      onEdit={() => actor && onEditActor(actor)}
                      onRemove={() => actor && onRemoveActor(actor, activeTab)}
                      onInbox={() => actor && onOpenActorInbox(actor)}
                      onStatusChange={onRefreshActors}
                    />
                  </ErrorBoundary>
                </div>
              </RuntimeInspectorModal>
            );
          })}
        </div>
      </main>

      <CodexVoiceOverlays voice={codexVoice} isDark={isDark} isSmallScreen={isSmallScreen} />
    </div>
  );
}
