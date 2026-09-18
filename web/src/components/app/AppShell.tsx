import { requestWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { WorkspaceNavigationDialog } from "../workspace/WorkspaceNavigationDialog";
import { useState, type CSSProperties } from "react";
import { ErrorBoundary } from "../ErrorBoundary";
import { AppHeader } from "../layout/AppHeader";
import { GroupConnectionsControl } from "../../features/connect/GroupConnectionsControl";
import { useModalStore } from "../../stores/useModalStore";
import { GroupSidebar } from "../layout/GroupSidebar";
import type { GroupControl } from "../../utils/groupControls";
import {
  CodexVoiceMobileDock,
  CodexVoiceOverlays,
} from "../../features/codexVoice/CodexVoiceShellSurfaces";
import { useCodexVoiceShell } from "../../features/codexVoice/useCodexVoiceShell";
import { useVoiceViewedMessages } from "../../features/codexVoice/useVoiceViewedMessages";
import { useUIStore } from "../../stores";
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
import {
  getSidebarWidthCssValue,
  SIDEBAR_COLLAPSED_WIDTH,
  groupMessagesVisible,
} from "../../stores/useUIStore";
import type { ComposerMentionKind } from "../../pages/chat/chatMentionSuggestions";
import type { ConnectWorkbench } from "../../features/connect/useConnectWorkbench";
type AppShellProps = {
  connectEmbedded?: boolean;
  connect?: ConnectWorkbench;
  remoteWorkspace?: React.ReactNode;
  canUseVoice?: boolean;
  onOpenVoiceSource?: (groupId: string, eventId: string) => void;
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
  onControlGroup: (groupId: string, control: GroupControl) => void;
  onDeleteGroup: (groupId: string) => void;
  onOpenSidebar: () => void;
  onOpenGroupEdit: (() => void) | undefined;
  onOpenSearch: () => void;
  onOpenContext: () => void;
  onStartGroup: () => void;
  onOpenSettings: () => void;
  canAccessAccount: boolean;
  accountLabel?: string | null;
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

export function AppShell({
  connectEmbedded = false,
  connect,
  remoteWorkspace,
  canUseVoice = false,
  onOpenVoiceSource,
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
  onControlGroup,
  onDeleteGroup,
  onOpenSidebar,
  onOpenGroupEdit,
  onOpenSearch,
  onOpenContext,
  onStartGroup,
  onOpenSettings,
  canAccessAccount,
  accountLabel,
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
  const shellStyle = {
    "--sidebar-width": sidebarCollapsed
      ? `${SIDEBAR_COLLAPSED_WIDTH}px`
      : getSidebarWidthCssValue(sidebarWidth),
  } as CSSProperties;
  const setGroupConnections = useModalStore((state) => state.setGroupConnections);
  const [workControlsHost, setWorkControlsHost] = useState<HTMLDivElement | null>(null);
  const [sidePanelControlsHost, setSidePanelControlsHost] = useState<HTMLDivElement | null>(null);
  const messagesVisible = useUIStore((state) => groupMessagesVisible(selectedGroupId, state));
  const codexVoice = useCodexVoiceShell(!webReadOnly && canUseVoice);
  useVoiceViewedMessages(
    contentRef,
    selectedGroupId,
    !webReadOnly && canUseVoice && messagesVisible && !remoteWorkspace,
  );

  return (
    <div
      className={`relative h-full min-h-0 transition-[grid-template-columns] duration-300 ease-out ${connectEmbedded ? "" : "md:grid md:[grid-template-columns:var(--sidebar-width)_minmax(0,1fr)]"}`}
      style={shellStyle}
    >
      {!connectEmbedded ? (
        <GroupSidebar
          connect={connect}
          orderedGroups={orderedGroups}
          archivedGroupIds={archivedGroupIds}
          selectedGroupId={remoteWorkspace ? "" : selectedGroupId}
          isOpen={sidebarOpen}
          isCollapsed={sidebarCollapsed}
          sidebarWidth={sidebarWidth}
          isDark={isDark}
          readOnly={webReadOnly}
          codexVoice={canUseVoice ? codexVoice : undefined}
          onSelectGroup={(groupId) => {
            if (!remoteWorkspace && groupId === selectedGroupId) onSelectGroup(groupId);
            else requestWorkspaceNavigation(() => onSelectGroup(groupId));
          }}
          onWarmGroup={onWarmGroup}
          onCreateGroup={onCreateGroup}
          onClose={onCloseSidebar}
          onToggleCollapse={onToggleSidebar}
          onResizeWidth={onResizeSidebar}
          onReorderSection={onReorderGroupsInSection}
          onArchiveGroup={onArchiveGroup}
          onRestoreGroup={onRestoreGroup}
          onControlGroup={webReadOnly ? undefined : onControlGroup}
          onDeleteGroup={webReadOnly || !canAccessAccount ? undefined : onDeleteGroup}
          onOpenGroupConnections={
            !webReadOnly && canAccessAccount ? setGroupConnections : undefined
          }
        />
      ) : null}

      <main
        data-group-shell
        className="absolute inset-0 flex h-full min-h-0 flex-col overflow-hidden md:relative md:inset-auto bg-transparent md:bg-[var(--color-chat-bg)]"
      >
        {remoteWorkspace || (
          <>
            <AppHeader
              workControlsRef={setWorkControlsHost}
              sidePanelControlsRef={setSidePanelControlsHost}
              theme={theme}
              textScale={textScale}
              onThemeChange={onThemeChange}
              onTextScaleChange={onTextScaleChange}
              webReadOnly={webReadOnly}
              selectedGroupId={selectedGroupId}
              groupDoc={groupDoc}
              selectedGroupRunning={selectedGroupRunning}
              selectedGroupRuntimeStatus={selectedGroupRuntimeStatus}
              sseStatus={sseStatus}
              onOpenSidebar={onOpenSidebar}
              onOpenGroupEdit={onOpenGroupEdit}
              onOpenSearch={onOpenSearch}
              onOpenContext={onOpenContext}
              onControlGroup={webReadOnly ? undefined : onControlGroup}
              onOpenSettings={onOpenSettings}
              canAccessAccount={canAccessAccount}
              accountLabel={accountLabel}
              onOpenAccount={onOpenAccount}
              onOpenMobileMenu={onOpenMobileMenu}
            />

            {!webReadOnly && canUseVoice ? <CodexVoiceMobileDock voice={codexVoice} /> : null}

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
                    workControlsHost={workControlsHost}
                    sidePanelControlsHost={sidePanelControlsHost}
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
                    renderRuntimeActor={(actorId, view) => {
                      const actor = runtimeActors.find((item) => item.id === actorId) || null;
                      return (
                        <ActorTab
                          actor={actor}
                          groupId={selectedGroupId}
                          agentState={
                            (groupContext?.agent_states || []).find(
                              (item) => item.id === actorId,
                            ) || null
                          }
                          termEpoch={getTermEpoch(actorId)}
                          busy={busy}
                          isDark={isDark}
                          isSmallScreen={isSmallScreen}
                          isVisible={view.isVisible}
                          compact={view.compact}
                          onExpand={view.onExpand}
                          navigation={view.navigation}
                          onPage={view.onPage}
                          readOnly={webReadOnly}
                          actorStatusProvisional={selectedGroupActorStatusProvisional}
                          onToggleEnabled={(running) =>
                            actor && onToggleActorEnabled(actor, running)
                          }
                          onRelaunch={() => actor && onRelaunchActor(actor)}
                          onNewSession={() => actor && onNewActorSession(actor)}
                          onEdit={() => actor && onEditActor(actor)}
                          onRemove={() => actor && onRemoveActor(actor, activeTab)}
                          onInbox={() => actor && onOpenActorInbox(actor)}
                          onStatusChange={onRefreshActors}
                        />
                      );
                    }}
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
            </div>
          </>
        )}
      </main>

      <WorkspaceNavigationDialog />
      <GroupConnectionsControl
        enabled={!webReadOnly && canAccessAccount}
        groupId={selectedGroupId}
        groups={orderedGroups}
        onOpenAccount={onOpenAccount}
      />
      <CodexVoiceOverlays
        voice={codexVoice}
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        onOpenSource={onOpenVoiceSource}
      />
    </div>
  );
}
