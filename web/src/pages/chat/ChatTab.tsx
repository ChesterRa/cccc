// ChatTab is the main chat page component.
// Refactored to use useChatTab hook for business logic, reducing prop drilling.

import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type MutableRefObject,
  type RefObject,
} from "react";
import { InfoIcon } from "../../components/Icons";
import {
  Actor,
  LedgerEvent,
  PresentationMessageRef,
  TaskMessageRef,
  VoiceDocumentMessageRef,
  isBrowserWebModelRuntime,
} from "../../types";
import { VirtualMessageList } from "../../components/VirtualMessageList";
import { getReadingTopAnchorOffset } from "../../components/virtualMessageListAnchorRestore";
import { PendingReplyEditingContext } from "../../components/messageBubble/PendingReplyEditingContext";
import { LivePendingStatusContext } from "../../features/pendingReplies/liveStatusContext";
import { useWaitingFilterIndicator } from "../../features/pendingReplies/useWaitingFilterIndicator";
import { useLivePendingStatuses } from "../../features/pendingReplies/useLivePendingStatuses";
import { NotificationPrompt } from "../../features/pushNotifications/NotificationPrompt";
import { useNotificationPromptOffer } from "../../features/pushNotifications/useNotificationPromptOffer";
import { classNames } from "../../utils/classNames";
import { buttonVariants } from "../../components/ui/button-variants";
import { ChatComposer } from "./ChatComposer";
import { ComposerFailedSends } from "./ComposerFailedSends";
import { ChatFilterTabs } from "./ChatFilterTabs";
import { usePinnedIds } from "../../features/pins/pinStore";
import { GroupWorkArea, type RuntimeActorRenderer } from "./GroupWorkArea";
import { RuntimeDock } from "./RuntimeDock";
import { ChatSenderFilter } from "./ChatSenderFilter";
import { pinnedEmptyKey } from "../../hooks/chat/chatMessageFilters";
import { useChatTab } from "../../hooks/useChatTab";
import { useTranslation } from "react-i18next";
import { useShallow } from "zustand/react/shallow";
import { useComposerStore, useGroupStore, useModalStore, useUIStore } from "../../stores";
import { getChatSession } from "../../stores/useUIStore";
import { findPresentationSlot } from "../../utils/presentation";
import { buildPresentationRefForSlot } from "../../utils/presentationRefs";
import { clearPresentationSlot } from "../../services/api";
import { ResizableSidePanel } from "../../components/layout/ResizableSidePanel";
import { MOBILE_APP_HEADER_HEIGHT_PX } from "../../utils/responsiveLayout";
import { useShortTouchViewport } from "../../hooks/useShortTouchViewport";
import { useMobileReadingChrome } from "../../hooks/useMobileReadingChrome";
import { useKeepInlineReplyVisible } from "../../hooks/useKeepInlineReplyVisible";
import {
  buildWebModelDeliveryStatusByEventId,
  latestWebModelDeliveryStatusNeedingAppPermissionHint,
} from "../../utils/webModelDeliveryStatus";
import {
  CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT,
  dismissChatGptAppPermissionHint,
  readChatGptAppPermissionHintDismissed,
} from "../../utils/chatGptAppPermissionHint";
import { useRuntimeDockWorkCards } from "./useRuntimeDockWorkCards";
import { useFloatingDockReserve } from "./useFloatingDockReserve";
import { type ComposerMentionKind } from "./chatMentionSuggestions";
import { useWorkspaceFiles } from "../../components/workspace/useWorkspaceFiles";
import { WorkspaceFilesTrigger } from "../../components/workspace/WorkspaceFilesTrigger";
import { useSidePanelSelection } from "../../hooks/useSidePanelSelection";
import { ensurePresentation } from "../../utils/presentation";
import { PresentationTrigger } from "../../components/presentation/PresentationTrigger";
import { MobilePresentationSurface } from "../../components/presentation/MobilePresentationSurface";

// The filter's 44px content row has a 1px bottom border.
const MOBILE_READING_CHROME_HEIGHT_PX = MOBILE_APP_HEADER_HEIGHT_PX + 45;

const PresentationRail = lazy(() =>
  import("../../components/presentation/PresentationRail").then((module) => ({
    default: module.PresentationRail,
  })),
);
const PresentationViewerSplitPanel = lazy(() =>
  import("../../components/presentation/PresentationViewerModal").then((module) => ({
    default: module.PresentationViewerSplitPanel,
  })),
);
const WorkspaceFilesPanel = lazy(() =>
  import("../../components/workspace/WorkspaceFilesPanel").then((module) => ({
    default: module.WorkspaceFilesPanel,
  })),
);
const WorkspaceViewer = lazy(() =>
  import("../../components/workspace/WorkspaceViewer").then((module) => ({
    default: module.WorkspaceViewer,
  })),
);
const SetupChecklist = lazy(() =>
  import("./SetupChecklist").then((module) => ({ default: module.SetupChecklist })),
);

const EMPTY_PRESENTATION_ATTENTION: Record<string, boolean> = {};
function ChatLazyFallback({ className }: { className?: string }) {
  return <div className={classNames("min-h-0", className)} />;
}

function ChatGptAppPermissionNotice({ onDismiss }: { onDismiss: () => void }) {
  const { t } = useTranslation("chat");
  // Framed like the notification offer above the composer, with the same key.
  return (
    <div className="flex-shrink-0 px-2 pb-1.5 sm:px-2.5">
      <div
        className="chat-reading-width mx-auto flex max-w-5xl items-start gap-2.5 rounded-lg border border-[var(--color-border-primary)] bg-[var(--color-bg-elevated)] px-3 py-2 text-xs leading-5 text-[var(--color-text-primary)] shadow-sm"
        role="status"
        aria-live="polite"
      >
        <InfoIcon size={16} className="mt-0.5 shrink-0" aria-hidden="true" />
        <div className="min-w-0 flex-1">{t("webModelDelivery.permissionHint")}</div>
        <button
          type="button"
          onClick={onDismiss}
          className={buttonVariants({ variant: "ghost", size: "sm" })}
          data-lcd-key
        >
          {t("webModelDelivery.permissionHintDismiss")}
        </button>
      </div>
    </div>
  );
}

export interface ChatTabProps {
  // UI configuration
  isDark: boolean;
  isSmallScreen: boolean;
  readOnly?: boolean;
  /** The viewer may create a Group, so the empty chat can offer it. */
  canCreateGroup?: boolean;
  mobileAppHeaderReserved?: boolean;
  onMobileReadingHiddenChange?: (hidden: boolean) => void;
  onMobileReadingOverlayChange?: (overlay: boolean) => void;

  // Core data (must be passed from App)
  selectedGroupId: string;
  selectedGroupRunning: boolean;
  selectedGroupActorsHydrating: boolean;
  selectedGroupActorStatusProvisional: boolean;
  groupLabelById: Record<string, string>;
  actors: Actor[];
  runtimeActors: Actor[];
  activeRuntimeActorId?: string;
  renderRuntimeActor: RuntimeActorRenderer;
  workControlsHost?: HTMLElement | null;
  sidePanelControlsHost?: HTMLElement | null;

  // Recipient actors for cross-group messaging
  recipientActors: Actor[];
  recipientActorsBusy?: boolean;
  destGroupScopeLabel?: string;
  mentionFilter?: string;
  mentionKind?: ComposerMentionKind;
  mentionActorScope?: "selected" | "destination";

  // Refs (shared with App for external interactions)
  scrollRef: MutableRefObject<HTMLDivElement | null>;
  composerRef: RefObject<HTMLTextAreaElement | null>;
  fileInputRef: RefObject<HTMLInputElement | null>;

  // Refs for scroll state (shared with App)
  chatAtBottomRef?: MutableRefObject<boolean>;

  // File handling (from useDragDrop)
  appendComposerFiles: (files: File[]) => void;

  // Group actions (from useGroupActions)
  onStartGroup: () => void;
  onOpenRuntimeActor: (actorId: string) => void;

  // Mention menu state (local state in App)
  showMentionMenu: boolean;
  setShowMentionMenu: React.Dispatch<React.SetStateAction<boolean>>;
  mentionSelectedIndex: number;
  setMentionSelectedIndex: React.Dispatch<React.SetStateAction<number>>;
  setMentionFilter: React.Dispatch<React.SetStateAction<string>>;
  setMentionKind: React.Dispatch<React.SetStateAction<ComposerMentionKind>>;
  setMentionActorScope: React.Dispatch<React.SetStateAction<"selected" | "destination">>;
  setMentionTargetGroupId: React.Dispatch<React.SetStateAction<string>>;
}

export function ChatTab({
  isDark,
  isSmallScreen,
  readOnly,
  canCreateGroup = false,
  mobileAppHeaderReserved = false,
  onMobileReadingHiddenChange,
  onMobileReadingOverlayChange,
  selectedGroupId,
  selectedGroupRunning,
  selectedGroupActorsHydrating,
  selectedGroupActorStatusProvisional,
  groupLabelById,
  actors,
  runtimeActors,
  activeRuntimeActorId,
  renderRuntimeActor,
  workControlsHost = null,
  sidePanelControlsHost = null,
  recipientActors,
  recipientActorsBusy,
  destGroupScopeLabel,
  mentionFilter,
  mentionKind,
  mentionActorScope,
  scrollRef,
  composerRef,
  fileInputRef,
  chatAtBottomRef,
  appendComposerFiles,
  onStartGroup,
  onOpenRuntimeActor,
  showMentionMenu,
  setShowMentionMenu,
  mentionSelectedIndex,
  setMentionSelectedIndex,
  setMentionFilter,
  setMentionKind,
  setMentionActorScope,
  setMentionTargetGroupId,
}: ChatTabProps) {
  const shortTouchViewport = useShortTouchViewport();
  const [composerInputFocused, setComposerInputFocused] = useState(false);
  const [mentionTriggerIndex, setMentionTriggerIndex] = useState<number | null>(null);
  const [inlineReplyEditor, setInlineReplyEditor] = useState<string | null>(null);
  const updateInlineReplyFocus = useCallback((editorId: string, focused: boolean) => {
    setInlineReplyEditor((current) => (focused ? editorId : current === editorId ? null : current));
  }, []);
  const [shortVisualViewport, setShortVisualViewport] = useState(
    () =>
      typeof window !== "undefined" && (window.visualViewport?.height ?? window.innerHeight) < 500,
  );
  useEffect(() => {
    const viewport = window.visualViewport;
    const update = () => setShortVisualViewport((viewport?.height ?? window.innerHeight) < 500);
    update();
    viewport?.addEventListener("resize", update);
    window.addEventListener("resize", update);
    return () => {
      viewport?.removeEventListener("resize", update);
      window.removeEventListener("resize", update);
    };
  }, []);
  const hideDockForKeyboard =
    (isSmallScreen || shortTouchViewport) &&
    (composerInputFocused || inlineReplyEditor !== null) &&
    shortVisualViewport &&
    !mobileAppHeaderReserved;
  // Use the refactored hook for business logic
  const {
    // Chat state
    chatMessages,
    suggestionSourceMessages,
    liveWorkEvents,
    hasAnyChatMessages,
    chatFilter,
    setChatFilter,
    chatSenderFilter,
    setChatSenderFilter,
    senderHistoryError,
    retrySenderHistory,
    pendingPageError,
    pendingDaemonUnsupported,
    retryPendingPage,
    pendingActive,
    pinnedActive,
    pinnedError,
    pinnedUnavailable,
    retryPinned,
    pinnedTabOffered,
    toggleMessagePin,
    viewMessageContext,
    chatViewKey,
    chatWindowProps,
    chatInitialScrollTargetId,
    chatInitialScrollAnchorId,
    chatInitialScrollAnchorOffsetPx,
    chatInitialScrollOffsetPx,
    chatInitialScrollHighlight,
    liveJumpActive,
    chatHighlightEventId,
    isLoadingHistory,
    hasMoreHistory,
    loadMoreHistory,
    chatEmptyState,

    // UI state
    busy,
    showScrollButton,
    chatUnreadCount,
    sendScrollRequest,
    consumeSendScrollRequest,

    // Setup checklist
    showSetupCard,
    needsScope,
    needsActors,
    needsStart,

    // Composer state
    setComposerText,
    composerGroupMentionTokens,
    setComposerGroupMentionTokens,
    composerAgentMentionTokens,
    setComposerAgentMentionTokens,
    composerFiles,
    removeComposerFile,
    replyTarget,
    quotedPresentationRef,
    quotedVoiceDocumentRef,
    cancelReply,
    clearQuotedPresentationRef,
    setQuotedVoiceDocumentRef,
    clearQuotedVoiceDocumentRef,
    toTokens,
    unavailableRecipients,
    toggleRecipient,
    clearRecipients,
    messageMode,
    setMessageMode,
    destGroupId,
    setDestGroupId,
    composerGroupSettled,
    composerRouteGroups,
    mentionSuggestions,
    connectMentionStatus,
    slashCommands,

    // Agent state
    agentStates,
    taskById,

    // Actions
    sendMessage,
    copyMessageLink,
    startReply,
    showRecipients,
    relayMessage,
    openSourceMessage,
    returnToLatest,
    handleScrollButtonClick,
    handleScrollChange,
    onContextReady,
    onReachedContextEnd,
    onLoadNewerContext,
    handleScrollSnapshot,
    addAgent,
  } = useChatTab({
    selectedGroupId,
    selectedGroupRunning,
    actors,
    recipientActors,
    showMentionMenu: showMentionMenu && !readOnly,
    mentionFilter,
    mentionKind,
    mentionActorScope,
    mentionTriggerIndex,
    composerRef,
    fileInputRef,
    chatAtBottomRef,
    scrollRef,
  });

  const messageGroupLabelById = groupLabelById;

  const { t } = useTranslation("chat");
  const groupPresentation = useGroupStore((state) => state.groupPresentation);
  const setGroupPresentation = useGroupStore((state) => state.setGroupPresentation);
  const presentationViewer = useModalStore((state) => state.presentationViewer);
  const setPresentationViewer = useModalStore((state) => state.setPresentationViewer);
  const setPresentationPin = useModalStore((state) => state.setPresentationPin);
  const clearPresentationSlotAttention = useModalStore(
    (state) => state.clearPresentationSlotAttention,
  );
  const openContextTask = useModalStore((state) => state.openContextTask);
  const mobileSurface = useUIStore((state) =>
    selectedGroupId
      ? getChatSession(selectedGroupId, state.chatSessions).mobileSurface
      : "messages",
  );
  const workView = useUIStore((state) =>
    selectedGroupId ? getChatSession(selectedGroupId, state.chatSessions).workView : "messages",
  );
  const presentationDisplayMode = useUIStore((state) =>
    selectedGroupId
      ? getChatSession(selectedGroupId, state.chatSessions).presentationDisplayMode
      : "modal",
  );
  const setChatFilesPanelOpen = useUIStore((state) => state.setChatFilesPanelOpen);
  const setChatMobileSurface = useUIStore((state) => state.setChatMobileSurface);
  const setChatPresentationDockOpen = useUIStore((state) => state.setChatPresentationDockOpen);
  const setChatPresentationDisplayMode = useUIStore(
    (state) => state.setChatPresentationDisplayMode,
  );
  const showError = useUIStore((state) => state.showError);
  const setQuotedPresentationRef = useComposerStore((state) => state.setQuotedPresentationRef);
  const setComposerDestGroupId = useComposerStore((state) => state.setDestGroupId);
  const liveWorkBucket = useGroupStore(
    useShallow((state) => {
      const bucket = state.chatByGroup[String(selectedGroupId || "").trim()];
      if (!bucket) return undefined;
      return {
        events: bucket.events,
        chatWindow: bucket.chatWindow,
        latestActorPreviewByActorId: bucket.latestActorPreviewByActorId,
        previewSessionsByActorId: bucket.previewSessionsByActorId,
        latestActorTextByActorId: bucket.latestActorTextByActorId,
        latestActorActivitiesByActorId: bucket.latestActorActivitiesByActorId,
        replySessionsByPendingEventId: bucket.replySessionsByPendingEventId,
      };
    }),
  );
  const presentationAttention = useModalStore((state) =>
    selectedGroupId
      ? state.presentationAttention[selectedGroupId] || EMPTY_PRESENTATION_ATTENTION
      : EMPTY_PRESENTATION_ATTENTION,
  );
  const webModelDeliveryStatusByEventId = useMemo(
    () =>
      buildWebModelDeliveryStatusByEventId(
        [...(liveWorkBucket?.events || []), ...(liveWorkBucket?.chatWindow?.events || [])],
        new Set(
          actors
            .filter((actor) => isBrowserWebModelRuntime(actor.runtime))
            .map((actor) => actor.id),
        ),
      ),
    [actors, liveWorkBucket?.chatWindow?.events, liveWorkBucket?.events],
  );
  const [appPermissionHintDismissed, setAppPermissionHintDismissed] = useState(
    readChatGptAppPermissionHintDismissed,
  );
  useEffect(() => {
    if (typeof window === "undefined") return undefined;
    const handleDismissed = () => setAppPermissionHintDismissed(true);
    window.addEventListener(CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT, handleDismissed);
    return () =>
      window.removeEventListener(CHATGPT_APP_PERMISSION_HINT_DISMISSED_EVENT, handleDismissed);
  }, []);
  const appPermissionHintStatus = useMemo(
    () =>
      latestWebModelDeliveryStatusNeedingAppPermissionHint(
        webModelDeliveryStatusByEventId,
        appPermissionHintDismissed,
      ),
    [appPermissionHintDismissed, webModelDeliveryStatusByEventId],
  );
  const showAppPermissionNotice = !readOnly && !!appPermissionHintStatus;
  const dismissAppPermissionNotice = useCallback(() => {
    setAppPermissionHintDismissed(true);
    dismissChatGptAppPermissionHint();
  }, []);

  const isHydratingEmptyState = chatMessages.length === 0 && chatEmptyState === "hydrating";
  // Pending pages already carry authoritative statuses for all retained rows.
  // Only the ordinary conversation needs a separate status hydration.
  const livePendingStatuses = useLivePendingStatuses(
    selectedGroupId,
    chatMessages,
    !readOnly && !pendingActive,
  );
  const isBusinessEmptyState = chatMessages.length === 0 && chatEmptyState === "business_empty";
  const isFilteredEmptyState = chatMessages.length === 0 && chatEmptyState === "filtered_empty";
  // Waiting for you and Pinned are read from the server rather than the loaded conversation.
  const projectionActive = pendingActive || pinnedActive;
  // The open Group's pins, read when it opens and again when the page becomes visible.
  const groupPins = usePinnedIds(selectedGroupId, pinnedTabOffered);
  const pinnedCount = groupPins?.loaded ? groupPins.ids.length : null;
  const pinnedEmpty = pinnedEmptyKey(pinnedError, pinnedCount);
  const listIsLoadingHistory = isLoadingHistory || isHydratingEmptyState;
  const listHasMoreHistory = hasMoreHistory || isHydratingEmptyState;
  const liveWorkCards = useRuntimeDockWorkCards({
    actors: runtimeActors,
    events: liveWorkEvents,
    bucket: liveWorkBucket,
  });

  const preferredPresentationSurface =
    !isSmallScreen && presentationDisplayMode === "split" ? "split" : "modal";
  const splitPresentationViewer =
    !isSmallScreen &&
    presentationViewer?.groupId === selectedGroupId &&
    presentationViewer.surface === "split"
      ? presentationViewer
      : null;
  // One right-hand column, two surfaces. The activity rail selects which one occupies it.
  const { activeSidePanel, selectSidePanel } = useSidePanelSelection(selectedGroupId);
  const showSplitFiles = !isSmallScreen && activeSidePanel === "files";
  const showSplitPresentation = !isSmallScreen && activeSidePanel === "presentation";
  const showSplitSurface = showSplitPresentation || showSplitFiles;
  // A phone has no room for a side column, so the tree becomes its own full-screen surface.
  const showMobileFiles = isSmallScreen && mobileSurface === "files" && !!selectedGroupId;
  // The tree lives in the right column while the opened file takes the main area, so the
  // controller is owned here rather than inside the panel.
  const workspaceScope = useGroupStore((state) =>
    state.groupDoc?.group_id === selectedGroupId
      ? state.groupDoc.scopes?.find((scope) => scope.scope_key === state.groupDoc?.active_scope_key)
      : undefined,
  );
  const workspaceFiles = useWorkspaceFiles(
    selectedGroupId,
    showSplitFiles || showMobileFiles,
    workspaceScope?.scope_key || "",
    workspaceScope?.url || "",
  );
  const hasWorkspaceViewer =
    workspaceFiles.mode === "changes" ? !!workspaceFiles.changes.selection : !!workspaceFiles.file;
  const showWorkspaceFileViewer = showSplitFiles && hasWorkspaceViewer;
  const setWorkspaceFileViewerGroupId = useUIStore((state) => state.setWorkspaceFileViewerGroupId);
  useLayoutEffect(() => {
    if (!showWorkspaceFileViewer) return;
    // The editor covers the message area. Do not mark hidden messages as viewed
    // or suppress their Voice notifications while that overlay is present.
    setWorkspaceFileViewerGroupId(selectedGroupId);
    return () => setWorkspaceFileViewerGroupId("");
  }, [selectedGroupId, showWorkspaceFileViewer, setWorkspaceFileViewerGroupId]);

  const showMobilePresentationViewer =
    isSmallScreen &&
    presentationViewer?.groupId === selectedGroupId &&
    presentationViewer.surface !== "split";
  const splitLayoutRef = useRef<HTMLDivElement | null>(null);

  const openPresentationSlot = useCallback(
    (slotId: string) => {
      if (!selectedGroupId || !slotId) return;
      if (preferredPresentationSurface === "split") {
        setChatPresentationDockOpen(selectedGroupId, true);
        setChatFilesPanelOpen(selectedGroupId, false);
      }
      setPresentationViewer({
        groupId: selectedGroupId,
        slotId,
        surface: preferredPresentationSurface,
      });
    },
    [
      preferredPresentationSurface,
      selectedGroupId,
      setChatFilesPanelOpen,
      setChatPresentationDockOpen,
      setPresentationViewer,
    ],
  );

  const openPresentationRef = useCallback(
    (ref: PresentationMessageRef, event: LedgerEvent) => {
      if (!selectedGroupId) return;
      const slotId = String(ref.slot_id || "").trim();
      if (!slotId) return;
      if (preferredPresentationSurface === "split") {
        setChatPresentationDockOpen(selectedGroupId, true);
        setChatFilesPanelOpen(selectedGroupId, false);
      }
      setPresentationViewer({
        groupId: selectedGroupId,
        slotId,
        surface: preferredPresentationSurface,
        focusRef: ref,
        focusEventId: String(event.id || "").trim() || null,
      });
    },
    [
      preferredPresentationSurface,
      selectedGroupId,
      setChatFilesPanelOpen,
      setChatPresentationDockOpen,
      setPresentationViewer,
    ],
  );

  const openTaskRef = useCallback(
    (ref: TaskMessageRef, _event?: LedgerEvent) => {
      const taskId = String(ref.task_id || "").trim();
      if (!taskId) return;
      openContextTask(taskId);
    },
    [openContextTask],
  );

  const pinPresentationSlot = useCallback(
    (slotId: string) => {
      if (!selectedGroupId || !slotId || readOnly) return;
      setPresentationPin({ groupId: selectedGroupId, slotId });
    },
    [readOnly, selectedGroupId, setPresentationPin],
  );

  /** Drops a workspace-relative path into the composer so the next message can name the file. */
  const attachWorkspacePath = useCallback(
    (path: string) => {
      const reference = `\`${path}\``;
      setComposerText((previous) => {
        const trimmed = previous.replace(/\s+$/, "");
        return trimmed ? `${trimmed} ${reference} ` : `${reference} `;
      });
      composerRef.current?.focus();
    },
    [composerRef, setComposerText],
  );

  const pinWorkspacePath = useCallback(
    (path: string) => {
      if (!selectedGroupId || readOnly) return;
      const slots = ensurePresentation(groupPresentation).slots;
      const target = slots.find((slot) => !slot.card) || slots[0];
      if (!target) return;
      setPresentationPin({ groupId: selectedGroupId, slotId: target.slot_id, workspacePath: path });
    },
    [groupPresentation, readOnly, selectedGroupId, setPresentationPin],
  );

  const closeMobileFiles = useCallback(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    setChatMobileSurface(gid, "messages");
  }, [selectedGroupId, setChatMobileSurface]);

  const closeMobilePresentation = useCallback(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    setChatMobileSurface(gid, "messages");
    window.setTimeout(() => {
      document.querySelector<HTMLButtonElement>("[data-mobile-presentation-trigger]")?.focus();
    }, 0);
  }, [selectedGroupId, setChatMobileSurface]);

  const handleQuotePresentationReference = useCallback(
    (payload: { slotId: string; ref?: PresentationMessageRef | null }) => {
      const gid = String(selectedGroupId || "").trim();
      const normalizedSlotId = String(payload.slotId || "").trim();
      if (!gid || !normalizedSlotId) return;
      const slot = findPresentationSlot(groupPresentation, normalizedSlotId);
      const ref = payload.ref || buildPresentationRefForSlot(slot);
      if (!ref) {
        showError(
          t("presentationMissingCard", { defaultValue: "This presentation slot is empty." }),
        );
        return;
      }
      setQuotedPresentationRef(ref);
      setComposerDestGroupId(gid);
      setChatMobileSurface(gid, "messages");
      setPresentationViewer(null);
      window.setTimeout(() => composerRef.current?.focus(), 0);
    },
    [
      composerRef,
      groupPresentation,
      selectedGroupId,
      setChatMobileSurface,
      setComposerDestGroupId,
      setPresentationViewer,
      setQuotedPresentationRef,
      showError,
      t,
    ],
  );

  const handleQuoteVoiceDocumentReference = useCallback(
    (ref: VoiceDocumentMessageRef) => {
      const gid = String(selectedGroupId || "").trim();
      if (!gid || ref.group_id !== gid || !String(ref.document_path || "").trim()) return;
      setQuotedVoiceDocumentRef(ref);
      setComposerDestGroupId(gid);
      setChatMobileSurface(gid, "messages");
      window.setTimeout(() => composerRef.current?.focus(), 0);
    },
    [
      composerRef,
      selectedGroupId,
      setChatMobileSurface,
      setComposerDestGroupId,
      setQuotedVoiceDocumentRef,
    ],
  );

  const handleOpenPresentationWindow = useCallback(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || !splitPresentationViewer) return;
    setChatPresentationDisplayMode(gid, "modal");
    setPresentationViewer({ ...splitPresentationViewer, surface: "modal" });
  }, [
    selectedGroupId,
    setChatPresentationDisplayMode,
    setPresentationViewer,
    splitPresentationViewer,
  ]);

  const handleSplitReplaceSlot = useCallback(
    (slotId: string) => {
      const gid = String(selectedGroupId || "").trim();
      if (!gid || !slotId) return;
      setPresentationViewer(null);
      setPresentationPin({ groupId: gid, slotId });
    },
    [selectedGroupId, setPresentationPin, setPresentationViewer],
  );

  const handleSplitClearSlot = useCallback(
    async (slotId: string) => {
      const gid = String(selectedGroupId || "").trim();
      const normalized = String(slotId || "").trim();
      if (!gid || !normalized) return;
      const confirmed = window.confirm(
        t("presentationClearConfirm", {
          index: Number(normalized.replace("slot-", "") || 0) || normalized,
          defaultValue: `Clear ${normalized}?`,
        }),
      );
      if (!confirmed) return;
      const resp = await clearPresentationSlot(gid, normalized);
      if (!resp.ok) {
        showError(`${resp.error.code}: ${resp.error.message}`);
        return;
      }
      setGroupPresentation(resp.result.presentation);
      setPresentationViewer(null);
      setPresentationPin(null);
      clearPresentationSlotAttention(gid, normalized);
    },
    [
      clearPresentationSlotAttention,
      selectedGroupId,
      setGroupPresentation,
      setPresentationPin,
      setPresentationViewer,
      showError,
      t,
    ],
  );

  const waitingIndicator = useWaitingFilterIndicator(
    selectedGroupId,
    chatFilter === "pending_user",
  );
  const hasChatFilter = chatFilter !== "all" || chatSenderFilter.length > 0;
  const showMessageFilters =
    !readOnly &&
    !chatWindowProps &&
    (hasAnyChatMessages || hasChatFilter || waitingIndicator !== null);
  const showSetupNotice =
    !readOnly &&
    (needsScope || needsActors) &&
    chatMessages.length > 0 &&
    !(inlineReplyEditor !== null && hideDockForKeyboard);
  const notificationOffer = useNotificationPromptOffer((waitingIndicator?.count ?? 0) > 0);
  const messageFiltersRef = useRef<HTMLDivElement | null>(null);
  const focusActiveFilter = useCallback(() => {
    messageFiltersRef.current?.querySelector<HTMLElement>('[aria-pressed="true"]')?.focus();
  }, []);
  // The key that opened a message from a filtered view leaves with its row. Focus goes to
  // the opened view's way back, not into the message list, where it would end the brief
  // hold that keeps the message in place.
  const windowReturnKeyRef = useRef<HTMLButtonElement | null>(null);
  const showInConversation = useCallback(
    (eventId: string) => {
      viewMessageContext(eventId);
      requestAnimationFrame(() => {
        if (document.activeElement === document.body) windowReturnKeyRef.current?.focus();
      });
    },
    [viewMessageContext],
  );
  // Keep the scroll viewport fixed beneath the mobile header and filters, so
  // showing or hiding those controls cannot move the reader's anchor. The
  // overlay reserves a fixed height, so extra rows under the filters keep the
  // controls in the normal flow instead.
  const overlayMobileChrome =
    isSmallScreen &&
    !mobileAppHeaderReserved &&
    mobileSurface === "messages" &&
    workView === "messages" &&
    !activeRuntimeActorId &&
    !showWorkspaceFileViewer &&
    !showSetupNotice &&
    !senderHistoryError &&
    !pendingPageError &&
    !pinnedError &&
    !(pinnedUnavailable.length > 0 && toggleMessagePin) &&
    showMessageFilters;
  useLayoutEffect(() => {
    onMobileReadingOverlayChange?.(overlayMobileChrome);
    return () => onMobileReadingOverlayChange?.(false);
  }, [onMobileReadingOverlayChange, overlayMobileChrome]);
  const reachedLatestWhileHidden = useRef("");
  const rememberReachedLatest = useCallback(() => {
    reachedLatestWhileHidden.current = selectedGroupId;
  }, [selectedGroupId]);
  const reading = useMobileReadingChrome(
    overlayMobileChrome && inlineReplyEditor === null,
    selectedGroupId,
    scrollRef,
    onMobileReadingHiddenChange,
    rememberReachedLatest,
  );
  // The phone's members grow with a line of agent activity; the list's end room follows them.
  const [dockFade, setDockFade] = useState<HTMLDivElement | null>(null);
  useFloatingDockReserve(dockFade, scrollRef, isSmallScreen);
  useKeepInlineReplyVisible(
    scrollRef,
    (isSmallScreen || shortTouchViewport) && inlineReplyEditor !== null,
    overlayMobileChrome ? MOBILE_READING_CHROME_HEIGHT_PX : 0,
  );
  useLayoutEffect(() => {
    if (reading.hidden || !reachedLatestWhileHidden.current) return;
    const reachedGroup = reachedLatestWhileHidden.current;
    reachedLatestWhileHidden.current = "";
    if (reachedGroup !== selectedGroupId) return;
    const scroll = scrollRef.current;
    if (!scroll) return;
    // Restoring the Dock reduces the viewport after the user has reached the
    // end. Keep the newest message visible and resume the existing follow mode.
    scroll.scrollTop = scroll.scrollHeight;
    handleScrollButtonClick();
  }, [handleScrollButtonClick, reading.hidden, scrollRef, selectedGroupId]);

  return (
    <div className="flex flex-col h-full w-full overflow-hidden bg-transparent">
      {/* Group work view and the secondary side panel. */}
      <main className="flex flex-1 min-h-0 flex-col">
        <div ref={splitLayoutRef} className="relative flex min-h-0 flex-1">
          {!isSmallScreen || mobileSurface === "messages" ? (
            <section
              className="relative flex min-h-0 min-w-0 flex-1 flex-col"
              onTouchStartCapture={reading.onTouchStartCapture}
              onTouchMoveCapture={reading.onTouchMoveCapture}
              onTouchEndCapture={reading.onTouchEndCapture}
              onTouchCancelCapture={reading.onTouchCancelCapture}
              onCompositionStartCapture={reading.onCompositionStartCapture}
              onCompositionEndCapture={reading.onCompositionEndCapture}
              onScrollCapture={reading.onScrollCapture}
              onPointerDownCapture={(event) => {
                if (event.target instanceof Element && event.target.closest("footer")) {
                  reading.reveal();
                }
              }}
              onClickCapture={reading.onClickCapture}
              onFocusCapture={(event) => {
                reading.onFocusCapture(event);
                if (event.target instanceof HTMLTextAreaElement && event.target.closest("footer")) {
                  setComposerInputFocused(true);
                  reading.reveal();
                }
              }}
              onBlurCapture={(event) => {
                if (event.target instanceof HTMLTextAreaElement && event.target.closest("footer")) {
                  setComposerInputFocused(false);
                }
              }}
              style={{
                paddingTop:
                  isSmallScreen && !mobileAppHeaderReserved && !overlayMobileChrome
                    ? MOBILE_APP_HEADER_HEIGHT_PX
                    : undefined,
              }}
            >
              <div className="relative flex min-h-0 flex-1 flex-col" data-chat-work-surface>
                {(chatWindowProps || showSetupNotice) && (
                  <div
                    className="shrink-0"
                    inert={showWorkspaceFileViewer || undefined}
                    aria-hidden={showWorkspaceFileViewer || undefined}
                    data-chat-notices
                  >
                    {/* Jump-to window banner */}
                    {chatWindowProps && (
                      <div className="px-4 pt-4">
                        <div
                          className="flex items-center justify-between gap-3 border border-[var(--color-border-primary)] bg-[var(--color-bg-elevated)] px-4 py-3"
                          role="status"
                          aria-label={t("viewingMessage")}
                        >
                          <div className="min-w-0">
                            <div className="text-sm font-semibold text-[var(--color-text-primary)]">
                              {t("viewingMessage")}
                            </div>
                            <div className="mt-0.5 text-xs text-[var(--color-text-secondary)]">
                              {isLoadingHistory
                                ? t("loadingContext")
                                : chatWindowProps.hasMoreBefore
                                  ? t("contextTruncated")
                                  : chatWindowProps.hasMoreAfter
                                    ? t("contextNewerNotLoaded")
                                    : t("contextLoaded")}
                            </div>
                          </div>
                          <button
                            ref={windowReturnKeyRef}
                            type="button"
                            className={buttonVariants({ variant: "ghost", size: "sm" })}
                            data-lcd-key
                            onClick={(event) =>
                              returnToLatest(
                                event.detail === 0 ||
                                  document.activeElement === event.currentTarget,
                                hasChatFilter,
                              )
                            }
                          >
                            {t(hasChatFilter ? "returnToFilteredChat" : "returnToLatest")}
                          </button>
                        </div>
                      </div>
                    )}

                    {/* Compact setup card */}
                    {showSetupNotice && (
                      <div className="px-4 pt-3 pb-2">
                        <Suspense fallback={<ChatLazyFallback />}>
                          <SetupChecklist
                            isDark={isDark}
                            selectedGroupId={selectedGroupId}
                            busy={busy}
                            needsScope={needsScope}
                            needsActors={needsActors}
                            needsStart={false}
                            onAddAgent={addAgent}
                            onStartGroup={onStartGroup}
                            variant="compact"
                          />
                        </Suspense>
                      </div>
                    )}
                  </div>
                )}

                <GroupWorkArea
                  covered={showWorkspaceFileViewer}
                  availableGroupIds={Object.keys(groupLabelById)}
                  readOnly={readOnly}
                  groupId={selectedGroupId}
                  actors={runtimeActors}
                  activeActorId={activeRuntimeActorId}
                  isDark={isDark}
                  isVisible={!isSmallScreen || mobileSurface === "messages"}
                  loading={selectedGroupActorsHydrating}
                  onInspectActor={onOpenRuntimeActor}
                  renderActor={renderRuntimeActor}
                  workControlsHost={workControlsHost}
                  sidePanelControlsHost={sidePanelControlsHost}
                  isSmallScreen={isSmallScreen}
                  sidePanelControls={
                    !selectedGroupId ? undefined : !isSmallScreen ? (
                      <>
                        <WorkspaceFilesTrigger
                          active={activeSidePanel === "files"}
                          onToggle={() => selectSidePanel("files")}
                        />
                        <PresentationTrigger
                          presentation={groupPresentation}
                          attentionSlots={presentationAttention}
                          isDark={isDark}
                          isOpen={activeSidePanel === "presentation"}
                          onOpen={() => selectSidePanel("presentation")}
                        />
                      </>
                    ) : !chatWindowProps ? (
                      <PresentationTrigger
                        mobile
                        presentation={groupPresentation}
                        attentionSlots={presentationAttention}
                        isDark={isDark}
                        isOpen={mobileSurface === "presentation"}
                        onOpen={() => setChatMobileSurface(selectedGroupId, "presentation")}
                      />
                    ) : undefined
                  }
                >
                  {showMessageFilters && (
                    <div
                      className={classNames(
                        "shrink-0 border-b border-[var(--glass-border-subtle)] px-4",
                        overlayMobileChrome &&
                          "absolute inset-x-0 z-20 transition-transform duration-150 motion-reduce:transition-none",
                        reading.hidden && "pointer-events-none",
                      )}
                      style={
                        overlayMobileChrome
                          ? {
                              top: MOBILE_APP_HEADER_HEIGHT_PX,
                              transform: reading.hidden
                                ? `translateY(-${MOBILE_READING_CHROME_HEIGHT_PX}px)`
                                : undefined,
                              backgroundColor: "var(--color-bg-primary)",
                              backgroundImage:
                                "linear-gradient(var(--color-chat-bg), var(--color-chat-bg))",
                            }
                          : undefined
                      }
                      inert={reading.hidden || undefined}
                      aria-hidden={reading.hidden || undefined}
                      data-message-filters
                    >
                      <div
                        ref={messageFiltersRef}
                        className="chat-reading-width flex min-h-[44px] md:min-h-9 flex-nowrap md:flex-wrap items-center gap-x-2"
                        role="group"
                        aria-label={t("chatFilters")}
                      >
                        <ChatFilterTabs
                          filter={chatFilter}
                          onChange={setChatFilter}
                          waiting={waitingIndicator}
                          pinned={pinnedTabOffered ? { count: pinnedCount } : null}
                          isSmallScreen={isSmallScreen}
                        />
                        <ChatSenderFilter
                          value={chatSenderFilter}
                          actors={actors}
                          onChange={setChatSenderFilter}
                        />
                      </div>
                      {senderHistoryError ? (
                        <div
                          role="status"
                          className="chat-reading-width flex items-center gap-2 pb-2 text-xs text-[var(--color-text-secondary)]"
                        >
                          <span>{t("senderHistoryFailed")}</span>
                          <button
                            type="button"
                            className="min-h-9 underline"
                            onClick={() => void retrySenderHistory()}
                          >
                            {t("common:retry")}
                          </button>
                        </div>
                      ) : null}
                      {pinnedError ? (
                        <div
                          role="status"
                          className="chat-reading-width flex items-center gap-2 pb-2 text-xs text-[var(--color-text-secondary)]"
                        >
                          <span>{t("pins.loadFailed")}</span>
                          <button type="button" className="min-h-9 underline" onClick={retryPinned}>
                            {t("common:retry")}
                          </button>
                        </div>
                      ) : null}
                      {pinnedUnavailable.length && toggleMessagePin ? (
                        <div
                          role="status"
                          className="chat-reading-width flex items-center gap-2 pb-2 text-xs text-[var(--color-text-secondary)]"
                        >
                          <span>{t("pins.unavailable", { count: pinnedUnavailable.length })}</span>
                          <button
                            type="button"
                            className="min-h-9 underline"
                            onClick={() => {
                              for (const id of pinnedUnavailable) void toggleMessagePin(id, false);
                            }}
                          >
                            {t("pins.removeUnavailable")}
                          </button>
                        </div>
                      ) : null}
                      {pendingPageError ? (
                        <div
                          role="status"
                          className="chat-reading-width flex items-center gap-2 pb-2 text-xs text-[var(--color-text-secondary)]"
                        >
                          <span>
                            {t(
                              pendingDaemonUnsupported
                                ? "pendingReply.daemonUnsupported"
                                : "pendingReply.loadFailed",
                            )}
                          </span>
                          <button
                            type="button"
                            className="min-h-9 underline"
                            onClick={retryPendingPage}
                          >
                            {t("common:retry")}
                          </button>
                        </div>
                      ) : null}
                    </div>
                  )}

                  {isBusinessEmptyState && showSetupCard ? (
                    <div
                      ref={scrollRef}
                      className="flex-1 min-h-0 overflow-auto px-4 py-4 relative"
                      role="log"
                      aria-label={t("chatMessages")}
                    >
                      <div className="flex h-full flex-col items-center justify-center text-center pb-20">
                        <div
                          className={classNames(
                            "w-full max-w-md",
                            isDark ? "text-slate-200" : "text-gray-800",
                          )}
                          data-setup-checklist
                        >
                          {readOnly ? (
                            <div
                              className={classNames(
                                "text-sm",
                                isDark ? "text-slate-400" : "text-gray-600",
                              )}
                              data-status-tone="muted"
                            >
                              {t("noMessagesYet")}
                            </div>
                          ) : (
                            <Suspense fallback={<ChatLazyFallback />}>
                              <SetupChecklist
                                isDark={isDark}
                                selectedGroupId={selectedGroupId}
                                busy={busy}
                                needsScope={needsScope}
                                needsActors={needsActors}
                                needsStart={needsStart}
                                onAddAgent={addAgent}
                                onStartGroup={onStartGroup}
                                variant="full"
                              />
                            </Suspense>
                          )}
                        </div>
                      </div>
                    </div>
                  ) : (
                    <PendingReplyEditingContext.Provider value={updateInlineReplyFocus}>
                      <LivePendingStatusContext.Provider
                        value={pendingActive ? null : livePendingStatuses}
                      >
                        <VirtualMessageList
                          messages={chatMessages}
                          actors={actors}
                          agentStates={agentStates}
                          taskById={taskById}
                          isDark={isDark}
                          readOnly={readOnly}
                          groupId={selectedGroupId}
                          groupLabelById={messageGroupLabelById}
                          webModelDeliveryStatusByEventId={webModelDeliveryStatusByEventId}
                          viewKey={chatViewKey}
                          topInsetPx={overlayMobileChrome ? MOBILE_READING_CHROME_HEIGHT_PX : 0}
                          followOnViewChangeKey={projectionActive ? undefined : chatViewKey}
                          initialScrollTargetId={
                            projectionActive ? undefined : chatInitialScrollTargetId
                          }
                          // Waiting for you and Pinned open at their newest message unless given
                          // a message to keep. A message given no height starts at the reading top.
                          initialScrollAnchorId={
                            projectionActive
                              ? chatInitialScrollAnchorId || chatMessages.at(-1)?.id
                              : chatInitialScrollAnchorId
                          }
                          initialScrollAnchorOffsetPx={
                            chatInitialScrollAnchorOffsetPx ??
                            getReadingTopAnchorOffset(
                              overlayMobileChrome ? MOBILE_READING_CHROME_HEIGHT_PX : 0,
                            )
                          }
                          initialScrollOffsetPx={chatInitialScrollOffsetPx}
                          highlightInitialScrollAnchor={chatInitialScrollHighlight}
                          highlightEventId={chatHighlightEventId}
                          liveJumpActive={liveJumpActive}
                          contextWindow={!!chatWindowProps}
                          className={
                            // Room to scroll the last message clear of the members over the
                            // list. It stays while reading hides them, so hiding them never
                            // moves the list, and goes only for a keyboard (chatMessage.css).
                            !chatWindowProps && runtimeActors.length > 0 && !hideDockForKeyboard
                              ? classNames(
                                  "chat-list-over-dock",
                                  !reading.hidden && "chat-dock-shown",
                                )
                              : undefined
                          }
                          scrollRef={scrollRef}
                          onReply={startReply}
                          onShowRecipients={showRecipients}
                          onCopyLink={copyMessageLink}
                          onTogglePin={toggleMessagePin}
                          onViewContext={
                            hasChatFilter && !chatWindowProps ? showInConversation : undefined
                          }
                          onRelay={relayMessage}
                          onOpenSource={openSourceMessage}
                          onOpenPresentationRef={openPresentationRef}
                          onOpenTaskRef={openTaskRef}
                          // The jump key floats over the list's corner; while an answer is being
                          // written it would cover that card's send key, so it waits.
                          showScrollButton={
                            showScrollButton &&
                            !(pendingActive && isSmallScreen) &&
                            inlineReplyEditor === null
                          }
                          onScrollButtonClick={handleScrollButtonClick}
                          chatUnreadCount={chatUnreadCount}
                          sendScrollRequest={sendScrollRequest}
                          onSendScrollRequestConsumed={consumeSendScrollRequest}
                          onScrollChange={handleScrollChange}
                          onContextReady={onContextReady}
                          onReachedContextEnd={onReachedContextEnd}
                          onLoadNewerContext={onLoadNewerContext}
                          isLoadingNewerContext={chatWindowProps?.isLoadingAfter}
                          newerContextError={chatWindowProps?.afterError}
                          onScrollSnapshot={handleScrollSnapshot}
                          isLoadingHistory={listIsLoadingHistory}
                          hasMoreHistory={listHasMoreHistory}
                          hasActiveFilters={hasChatFilter && !chatWindowProps}
                          isFilteredEmpty={isFilteredEmptyState}
                          emptyMessage={
                            !selectedGroupId
                              ? t(
                                  readOnly
                                    ? "noGroupSelectedReadOnly"
                                    : canCreateGroup
                                      ? "noGroupSelected"
                                      : "noGroupSelectedNoCreate",
                                )
                              : pendingActive
                                ? t(
                                    pendingDaemonUnsupported
                                      ? "pendingReply.daemonUnsupported"
                                      : pendingPageError
                                        ? "pendingReply.loadFailed"
                                        : "pendingReply.empty",
                                  )
                                : pinnedActive && pinnedEmpty
                                  ? t(pinnedEmpty)
                                  : undefined
                          }
                          onLoadMore={loadMoreHistory}
                        />
                      </LivePendingStatusContext.Provider>
                    </PendingReplyEditingContext.Provider>
                  )}

                  {!chatWindowProps && runtimeActors.length > 0 ? (
                    !isSmallScreen && workView === "messages" ? (
                      // Only the member controls catch input. The list keeps its full
                      // viewport and can scroll its final message above this overlay; text
                      // passing under it fades out behind the members' keys (chatMessage.css).
                      <div
                        className={classNames(
                          "pointer-events-none absolute inset-x-0 bottom-0 z-20 px-2 sm:px-4",
                          // A touch screen's keyboard leaves little height; the input needs it.
                          hideDockForKeyboard && "hidden",
                        )}
                      >
                        <div className="chat-reading-width">
                          <div className="chat-dock-fade">
                            <RuntimeDock
                              placement="row"
                              motionVisible={!hideDockForKeyboard}
                              groupId={selectedGroupId}
                              runtimeActors={runtimeActors}
                              liveWorkCards={liveWorkCards}
                              activeRuntimeActorId={activeRuntimeActorId}
                              isDark={isDark}
                              isSmallScreen={isSmallScreen}
                              readOnly={readOnly}
                              actorStatusProvisional={selectedGroupActorStatusProvisional}
                              onAddAgent={!readOnly ? addAgent : undefined}
                              onOpenRuntimeActor={onOpenRuntimeActor}
                            />
                          </div>
                        </div>
                      </div>
                    ) : (
                      <div
                        className={classNames(
                          "pointer-events-none absolute inset-x-0 z-20",
                          // On a phone the dock floats over the message list; elsewhere it
                          // floats over the work area's panes, which keep their own ground.
                          isSmallScreen ? "bottom-0" : "bottom-2",
                          (hideDockForKeyboard || reading.hidden) && "hidden",
                        )}
                        data-runtime-dock-keyboard-hidden={hideDockForKeyboard || undefined}
                      >
                        {/* Hidden by display, its rings restart on return; this lets them
                            take the shared phase again instead of their old offsets. */}
                        <div
                          ref={setDockFade}
                          className={isSmallScreen ? "chat-dock-fade" : undefined}
                        >
                          <RuntimeDock
                            motionVisible={!(hideDockForKeyboard || reading.hidden)}
                            groupId={selectedGroupId}
                            runtimeActors={runtimeActors}
                            liveWorkCards={liveWorkCards}
                            activeRuntimeActorId={activeRuntimeActorId}
                            isDark={isDark}
                            isSmallScreen={isSmallScreen}
                            readOnly={readOnly}
                            actorStatusProvisional={selectedGroupActorStatusProvisional}
                            onAddAgent={!readOnly ? addAgent : undefined}
                            onOpenRuntimeActor={onOpenRuntimeActor}
                          />
                        </div>
                      </div>
                    )
                  ) : null}
                </GroupWorkArea>

                {/* `--color-chat-bg` is a 75% glass tint, so it alone would let the chat
                  underneath show through. The overlay composites it over the opaque page
                  background to match the chat surface exactly while staying fully opaque. */}
                {showWorkspaceFileViewer ? (
                  <div
                    className="absolute inset-0 z-20 flex min-h-0 flex-col"
                    style={{
                      backgroundColor: "var(--color-bg-primary)",
                      backgroundImage:
                        "linear-gradient(var(--color-chat-bg), var(--color-chat-bg))",
                    }}
                    data-workspace-file-viewer="true"
                  >
                    <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                      <WorkspaceViewer
                        files={workspaceFiles}
                        isDark={isDark}
                        readOnly={!!readOnly}
                        onAttach={attachWorkspacePath}
                      />
                    </Suspense>
                  </div>
                ) : null}
              </div>
              {!readOnly && (
                <>
                  {showAppPermissionNotice ? (
                    <ChatGptAppPermissionNotice onDismiss={dismissAppPermissionNotice} />
                  ) : null}
                  {notificationOffer.visible && !hideDockForKeyboard ? (
                    <NotificationPrompt
                      onDismiss={notificationOffer.dismiss}
                      restoreFocus={focusActiveFilter}
                    />
                  ) : null}
                  {selectedGroupId ? (
                    <ChatComposer
                      isDark={isDark}
                      isSmallScreen={isSmallScreen}
                      keepExpanded={mobileAppHeaderReserved}
                      selectedGroupId={selectedGroupId}
                      actors={actors}
                      recipientActorsBusy={recipientActorsBusy}
                      selectedGroupActorsHydrating={selectedGroupActorsHydrating}
                      destGroupId={destGroupId}
                      setDestGroupId={setDestGroupId}
                      composerGroupSettled={composerGroupSettled}
                      composerRouteGroups={composerRouteGroups}
                      destGroupScopeLabel={destGroupScopeLabel}
                      busy={busy}
                      recentMessages={chatMessages}
                      suggestionSourceMessages={suggestionSourceMessages}
                      replyTarget={replyTarget}
                      onCancelReply={cancelReply}
                      quotedPresentationRef={quotedPresentationRef}
                      onClearQuotedPresentationRef={clearQuotedPresentationRef}
                      quotedVoiceDocumentRef={quotedVoiceDocumentRef}
                      onQuoteVoiceDocumentRef={handleQuoteVoiceDocumentReference}
                      onClearQuotedVoiceDocumentRef={clearQuotedVoiceDocumentRef}
                      toTokens={toTokens}
                      unavailableRecipients={unavailableRecipients}
                      onToggleRecipient={toggleRecipient}
                      onClearRecipients={clearRecipients}
                      composerFiles={composerFiles}
                      onRemoveComposerFile={removeComposerFile}
                      appendComposerFiles={appendComposerFiles}
                      fileInputRef={fileInputRef}
                      composerRef={composerRef}
                      setComposerText={setComposerText}
                      composerGroupMentionTokens={composerGroupMentionTokens}
                      setComposerGroupMentionTokens={setComposerGroupMentionTokens}
                      composerAgentMentionTokens={composerAgentMentionTokens}
                      setComposerAgentMentionTokens={setComposerAgentMentionTokens}
                      messageMode={messageMode}
                      setMessageMode={setMessageMode}
                      onSendMessage={sendMessage}
                      showMentionMenu={showMentionMenu}
                      setShowMentionMenu={setShowMentionMenu}
                      mentionSuggestions={mentionSuggestions}
                      connectMentionStatus={
                        mentionKind === "group" ? connectMentionStatus : undefined
                      }
                      mentionSelectedIndex={mentionSelectedIndex}
                      setMentionSelectedIndex={setMentionSelectedIndex}
                      setMentionFilter={setMentionFilter}
                      setMentionKind={setMentionKind}
                      setMentionActorScope={setMentionActorScope}
                      setMentionTargetGroupId={setMentionTargetGroupId}
                      setMentionTriggerIndex={setMentionTriggerIndex}
                      slashCommands={slashCommands}
                    />
                  ) : (
                    // With no Group open there is no composer, but sends waiting to be restored
                    // still need a place to be seen and discarded.
                    <div className="flex-shrink-0 px-2 pb-2 sm:px-4">
                      <div className="chat-reading-width">
                        <ComposerFailedSends groupId="" isDark={isDark} />
                      </div>
                    </div>
                  )}
                </>
              )}
            </section>
          ) : null}

          {showSplitSurface && activeSidePanel ? (
            <ResizableSidePanel
              groupId={selectedGroupId}
              surface={activeSidePanel}
              viewing={!!splitPresentationViewer}
              container={splitLayoutRef}
              isDark={isDark}
            >
              {({ compact, toggleCompact }) =>
                showSplitFiles ? (
                  <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
                      <WorkspaceFilesPanel
                        files={workspaceFiles}
                        isDark={isDark}
                        readOnly={!!readOnly}
                        // Routed through the selector so closing clears the column instead of
                        // letting the other surface pop up in its place.
                        onClose={() => selectSidePanel("files")}
                        onAttachPath={attachWorkspacePath}
                        onPinPath={pinWorkspacePath}
                      />
                    </div>
                  </Suspense>
                ) : splitPresentationViewer && !compact ? (
                  <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                    <PresentationViewerSplitPanel
                      key={`${selectedGroupId}:${splitPresentationViewer.slotId}`}
                      isDark={isDark}
                      readOnly={readOnly}
                      groupId={selectedGroupId}
                      slotId={splitPresentationViewer.slotId}
                      presentation={groupPresentation}
                      focusRef={splitPresentationViewer.focusRef || null}
                      focusEventId={splitPresentationViewer.focusEventId || null}
                      onQuoteInChat={handleQuotePresentationReference}
                      onReplaceSlot={handleSplitReplaceSlot}
                      onClearSlot={(slotId) => {
                        void handleSplitClearSlot(slotId);
                      }}
                      onOpenWindow={handleOpenPresentationWindow}
                      onSelectSlot={(slotId) =>
                        setPresentationViewer({
                          groupId: selectedGroupId,
                          slotId,
                          surface: "split",
                        })
                      }
                      onPinSlot={pinPresentationSlot}
                      onCollapse={() => {
                        toggleCompact();
                        requestAnimationFrame(() =>
                          document
                            .querySelector<HTMLButtonElement>(
                              '[data-presentation-density="compact"] button',
                            )
                            ?.focus(),
                        );
                      }}
                      onClose={() => selectSidePanel("presentation")}
                    />
                  </Suspense>
                ) : (
                  // Presentation with no slot opened: the slot list that phones already use.
                  <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
                      <PresentationRail
                        groupId={selectedGroupId}
                        compact={compact}
                        onToggleCompact={() => {
                          toggleCompact();
                          if (compact) {
                            const slots = ensurePresentation(groupPresentation);
                            const slot =
                              slots.slots.find(
                                (item) => item.slot_id === slots.highlight_slot_id && item.card,
                              ) || slots.slots.find((item) => item.card);
                            if (slot)
                              setPresentationViewer({
                                groupId: selectedGroupId,
                                slotId: slot.slot_id,
                                surface: "split",
                              });
                          }
                        }}
                        presentation={groupPresentation}
                        isDark={isDark}
                        readOnly={readOnly}
                        attentionSlots={presentationAttention}
                        onOpenSlot={openPresentationSlot}
                        onPinSlot={pinPresentationSlot}
                        onClose={() => selectSidePanel("presentation")}
                      />
                    </div>
                  </Suspense>
                )
              }
            </ResizableSidePanel>
          ) : null}

          <MobilePresentationSurface
            isOpen={showMobileFiles}
            isDark={isDark}
            surface="files"
            label={t("workspaceFilesTitle", { defaultValue: "Files" })}
            onClose={closeMobileFiles}
          >
            <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
              <div className="flex min-h-0 flex-1 flex-col">
                {hasWorkspaceViewer ? (
                  <WorkspaceViewer
                    files={workspaceFiles}
                    isDark={isDark}
                    readOnly
                    onAttach={(path) => {
                      attachWorkspacePath(path);
                      closeMobileFiles();
                    }}
                  />
                ) : (
                  <WorkspaceFilesPanel
                    files={workspaceFiles}
                    isDark={isDark}
                    readOnly
                    onClose={closeMobileFiles}
                    onAttachPath={(path) => {
                      attachWorkspacePath(path);
                      closeMobileFiles();
                    }}
                  />
                )}
              </div>
            </Suspense>
          </MobilePresentationSurface>

          <MobilePresentationSurface
            isOpen={
              isSmallScreen && mobileSurface === "presentation" && !showMobilePresentationViewer
            }
            isDark={isDark}
            label={t("presentationSectionLabel", { defaultValue: "Presentation" })}
            onClose={closeMobilePresentation}
          >
            <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
              <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                <PresentationRail
                  groupId={selectedGroupId}
                  presentation={groupPresentation}
                  isDark={isDark}
                  readOnly={readOnly}
                  onClose={closeMobilePresentation}
                  attentionSlots={presentationAttention}
                  onOpenSlot={openPresentationSlot}
                  onPinSlot={pinPresentationSlot}
                />
              </Suspense>
            </div>
          </MobilePresentationSurface>
        </div>
      </main>
    </div>
  );
}
