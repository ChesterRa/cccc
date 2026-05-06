// ChatTab is the main chat page component.
// Refactored to use useChatTab hook for business logic, reducing prop drilling.

import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState, type MutableRefObject, type RefObject } from "react";
import { BookmarkIcon, CompassIcon } from "../../components/Icons";
import { Actor, GroupMeta, HeadlessPreviewSession, LedgerEvent, PresentationMessageRef, StreamingActivity, TaskMessageRef } from "../../types";
import { VirtualMessageList } from "../../components/VirtualMessageList";
import { classNames } from "../../utils/classNames";
import { ChatComposer } from "./ChatComposer";
import { RuntimeDock } from "./RuntimeDock";
import { useChatTab } from "../../hooks/useChatTab";
import { useTranslation } from 'react-i18next';
import { useComposerStore, useGroupStore, useModalStore, useUIStore } from "../../stores";
import { getChatSession } from "../../stores/useUIStore";
import { findPresentationSlot } from "../../utils/presentation";
import { buildPresentationRefForSlot } from "../../utils/presentationRefs";
import { clearPresentationSlot } from "../../services/api";
import { clampPresentationSplitWidth } from "../../utils/presentationSplitLayout";
import {
  getMobileMessageTopInsetPx,
  MOBILE_FLOATING_CONTROLS_TOP_INSET_PX,
} from "../../utils/responsiveLayout";
import type { StreamingReplySession } from "../../stores/chatStreamingSessions";
import { buildLiveWorkCards } from "./liveWorkCards";

const PresentationRail = lazy(() =>
  import("../../components/presentation/PresentationRail").then((module) => ({ default: module.PresentationRail }))
);
const PresentationViewerSplitPanel = lazy(() =>
  import("../../components/presentation/PresentationViewerModal").then((module) => ({ default: module.PresentationViewerSplitPanel }))
);
const SetupChecklist = lazy(() =>
  import("./SetupChecklist").then((module) => ({ default: module.SetupChecklist }))
);

const EMPTY_PRESENTATION_ATTENTION: Record<string, boolean> = {};
const EMPTY_LIVE_WORK_TEXT: Record<string, string> = {};
const EMPTY_LIVE_WORK_ACTIVITIES: Record<string, StreamingActivity[]> = {};
const EMPTY_LIVE_WORK_SESSIONS: Record<string, StreamingReplySession> = {};
const EMPTY_LIVE_WORK_PREVIEW_SESSIONS: Record<string, HeadlessPreviewSession[]> = {};
const EMPTY_LATEST_LIVE_WORK_PREVIEW: Record<string, HeadlessPreviewSession> = {};

function ChatLazyFallback({ className }: { className?: string }) {
  return <div className={classNames("min-h-0", className)} />;
}

export interface ChatTabProps {
  // UI configuration
  isDark: boolean;
  isSmallScreen: boolean;
  readOnly?: boolean;

  // Core data (must be passed from App)
  selectedGroupId: string;
  selectedGroupRunning: boolean;
  selectedGroupActorsHydrating: boolean;
  groupLabelById: Record<string, string>;
  actors: Actor[];
  runtimeActors: Actor[];
  groups: GroupMeta[];
  activeRuntimeActorId?: string;

  // Recipient actors for cross-group messaging
  recipientActors: Actor[];
  recipientActorsBusy?: boolean;
  destGroupScopeLabel?: string;

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
}

export function ChatTab({
  isDark,
  isSmallScreen,
  readOnly,
  selectedGroupId,
  selectedGroupRunning,
  selectedGroupActorsHydrating,
  groupLabelById,
  actors,
  runtimeActors,
  groups,
  activeRuntimeActorId,
  recipientActors,
  recipientActorsBusy,
  destGroupScopeLabel,
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
}: ChatTabProps) {
  // Use the refactored hook for business logic
  const {
    // Chat state
    chatMessages,
    liveWorkEvents,
    hasAnyChatMessages,
    chatFilter,
    setChatFilter,
    chatViewKey,
    chatWindowProps,
    chatInitialScrollTargetId,
    chatInitialScrollAnchorId,
    chatInitialScrollAnchorOffsetPx,
    chatHighlightEventId,
    isLoadingHistory,
    hasMoreHistory,
    loadMoreHistory,
    chatEmptyState,

    // UI state
    busy,
    showScrollButton,
    chatUnreadCount,
    forceStickToBottomToken,

    // Setup checklist
    showSetupCard,
    needsScope,
    needsActors,
    needsStart,

    // Composer state
    composerText,
    setComposerText,
    composerFiles,
    removeComposerFile,
    replyTarget,
    quotedPresentationRef,
    cancelReply,
    clearQuotedPresentationRef,
    toTokens,
    toggleRecipient,
    clearRecipients,
    appendRecipientToken,
    priority,
    replyRequired,
    setPriority,
    setReplyRequired,
    destGroupId,
    setDestGroupId,
    mentionSuggestions,

    // Agent state
    agentStates,
    taskById,

    // Actions
    sendMessage,
    copyMessageLink,
    copyMessageText,
    startReply,
    showRecipients,
    relayMessage,
    openSourceMessage,
    exitChatWindow,
    handleScrollButtonClick,
    handleScrollChange,
    handleScrollSnapshot,
    addAgent,
  } = useChatTab({
    selectedGroupId,
    selectedGroupRunning,
    actors,
    recipientActors,
    composerRef,
    fileInputRef,
    chatAtBottomRef,
    scrollRef,
  });

  const { t } = useTranslation('chat');
  const groupPresentation = useGroupStore((state) => state.groupPresentation);
  const setGroupPresentation = useGroupStore((state) => state.setGroupPresentation);
  const presentationViewer = useModalStore((state) => state.presentationViewer);
  const setPresentationViewer = useModalStore((state) => state.setPresentationViewer);
  const setPresentationPin = useModalStore((state) => state.setPresentationPin);
  const clearPresentationSlotAttention = useModalStore((state) => state.clearPresentationSlotAttention);
  const openContextTask = useModalStore((state) => state.openContextTask);
  const mobileSurface = useUIStore((state) =>
    selectedGroupId ? getChatSession(selectedGroupId, state.chatSessions).mobileSurface : "messages"
  );
  const presentationDockOpen = useUIStore((state) =>
    selectedGroupId ? getChatSession(selectedGroupId, state.chatSessions).presentationDockOpen : false
  );
  const presentationDisplayMode = useUIStore((state) =>
    selectedGroupId ? getChatSession(selectedGroupId, state.chatSessions).presentationDisplayMode : "modal"
  );
  const setChatMobileSurface = useUIStore((state) => state.setChatMobileSurface);
  const setChatPresentationDockOpen = useUIStore((state) => state.setChatPresentationDockOpen);
  const setChatPresentationDisplayMode = useUIStore((state) => state.setChatPresentationDisplayMode);
  const presentationSplitWidth = useUIStore((state) => state.presentationSplitWidth);
  const setPresentationSplitWidth = useUIStore((state) => state.setPresentationSplitWidth);
  const showError = useUIStore((state) => state.showError);
  const setQuotedPresentationRef = useComposerStore((state) => state.setQuotedPresentationRef);
  const setComposerDestGroupId = useComposerStore((state) => state.setDestGroupId);
  const liveWorkBucket = useGroupStore((state) => state.chatByGroup[String(selectedGroupId || "").trim()]);
  const presentationAttention = useModalStore((state) =>
    selectedGroupId ? (state.presentationAttention[selectedGroupId] || EMPTY_PRESENTATION_ATTENTION) : EMPTY_PRESENTATION_ATTENTION
  );
  const previewSessionsByActorId = liveWorkBucket?.previewSessionsByActorId ?? EMPTY_LIVE_WORK_PREVIEW_SESSIONS;
  const latestActorPreviewByActorId = liveWorkBucket?.latestActorPreviewByActorId ?? EMPTY_LATEST_LIVE_WORK_PREVIEW;
  const latestActorTextByActorId = liveWorkBucket?.latestActorTextByActorId || EMPTY_LIVE_WORK_TEXT;
  const latestActorActivitiesByActorId = liveWorkBucket?.latestActorActivitiesByActorId || EMPTY_LIVE_WORK_ACTIVITIES;
  const replySessionsByPendingEventId = liveWorkBucket?.replySessionsByPendingEventId || EMPTY_LIVE_WORK_SESSIONS;

  const isHydratingEmptyState = chatMessages.length === 0 && chatEmptyState === "hydrating";
  const isBusinessEmptyState = chatMessages.length === 0 && chatEmptyState === "business_empty";
  const listIsLoadingHistory = isLoadingHistory || isHydratingEmptyState;
  const listHasMoreHistory = hasMoreHistory || isHydratingEmptyState;
  const hasPresentationAttention = Object.keys(presentationAttention).length > 0;
  const liveWorkCards = useMemo(
    () => buildLiveWorkCards({
      actors: runtimeActors,
      events: liveWorkEvents,
      latestActorPreviewByActorId,
      previewSessionsByActorId,
      latestActorTextByActorId,
      latestActorActivitiesByActorId,
      replySessionsByPendingEventId,
    }),
    [
      runtimeActors,
      liveWorkEvents,
      latestActorPreviewByActorId,
      previewSessionsByActorId,
      latestActorActivitiesByActorId,
      latestActorTextByActorId,
      replySessionsByPendingEventId,
    ]
  );

  const preferredPresentationSurface = !isSmallScreen && presentationDisplayMode === "split" ? "split" : "modal";
  const splitPresentationViewer =
    !isSmallScreen &&
    presentationViewer?.groupId === selectedGroupId &&
    presentationViewer.surface === "split"
      ? presentationViewer
      : null;
  const showDesktopSplitPresentation = !!splitPresentationViewer;
  const splitLayoutRef = useRef<HTMLDivElement | null>(null);
  const splitResizeRef = useRef<{ startX: number; startWidth: number } | null>(null);
  const [isSplitResizing, setIsSplitResizing] = useState(false);
  const [splitLayoutWidth, setSplitLayoutWidth] = useState(0);
  const effectivePresentationSplitWidth = clampPresentationSplitWidth(
    presentationSplitWidth,
    splitLayoutWidth || undefined
  );

  useEffect(() => {
    const node = splitLayoutRef.current;
    if (!node) return undefined;

    const updateWidth = () => {
      setSplitLayoutWidth(node.clientWidth || 0);
    };

    updateWidth();
    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", updateWidth);
      return () => window.removeEventListener("resize", updateWidth);
    }

    const observer = new ResizeObserver(() => updateWidth());
    observer.observe(node);
    return () => observer.disconnect();
  }, [showDesktopSplitPresentation]);

  useEffect(() => {
    if (!isSplitResizing) return undefined;

    const handlePointerMove = (event: PointerEvent) => {
      const drag = splitResizeRef.current;
      if (!drag) return;
      const nextWidth = drag.startWidth - (event.clientX - drag.startX);
      const containerWidth = splitLayoutRef.current?.clientWidth || splitLayoutWidth || undefined;
      setPresentationSplitWidth(clampPresentationSplitWidth(nextWidth, containerWidth));
    };

    const finishResize = () => {
      splitResizeRef.current = null;
      setIsSplitResizing(false);
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
  }, [isSplitResizing, setPresentationSplitWidth, splitLayoutWidth]);

  const openPresentationSlot = useCallback((slotId: string) => {
    if (!selectedGroupId || !slotId) return;
    if (preferredPresentationSurface === "split") {
      setChatPresentationDockOpen(selectedGroupId, true);
    }
    setPresentationViewer({ groupId: selectedGroupId, slotId, surface: preferredPresentationSurface });
  }, [preferredPresentationSurface, selectedGroupId, setChatPresentationDockOpen, setPresentationViewer]);

  const openPresentationRef = useCallback((ref: PresentationMessageRef, event: LedgerEvent) => {
    if (!selectedGroupId) return;
    const slotId = String(ref.slot_id || "").trim();
    if (!slotId) return;
    if (preferredPresentationSurface === "split") {
      setChatPresentationDockOpen(selectedGroupId, true);
    }
    setPresentationViewer({
      groupId: selectedGroupId,
      slotId,
      surface: preferredPresentationSurface,
      focusRef: ref,
      focusEventId: String(event.id || "").trim() || null,
    });
  }, [preferredPresentationSurface, selectedGroupId, setChatPresentationDockOpen, setPresentationViewer]);

  const openTaskRef = useCallback((ref: TaskMessageRef, _event?: LedgerEvent) => {
    const taskId = String(ref.task_id || "").trim();
    if (!taskId) return;
    openContextTask(taskId);
  }, [openContextTask]);

  const pinPresentationSlot = useCallback((slotId: string) => {
    if (!selectedGroupId || !slotId || readOnly) return;
    setPresentationPin({ groupId: selectedGroupId, slotId });
  }, [readOnly, selectedGroupId, setPresentationPin]);

  const setPresentationDockOpen = useCallback((next: boolean) => {
    if (!selectedGroupId) return;
    setChatPresentationDockOpen(selectedGroupId, next);
  }, [selectedGroupId, setChatPresentationDockOpen]);

  const handleQuotePresentationReference = useCallback((payload: { slotId: string; ref?: PresentationMessageRef | null }) => {
    const gid = String(selectedGroupId || "").trim();
    const normalizedSlotId = String(payload.slotId || "").trim();
    if (!gid || !normalizedSlotId) return;
    const slot = findPresentationSlot(groupPresentation, normalizedSlotId);
    const ref = payload.ref || buildPresentationRefForSlot(slot);
    if (!ref) {
      showError(t("presentationMissingCard", { defaultValue: "This presentation slot is empty." }));
      return;
    }
    setQuotedPresentationRef(ref);
    setComposerDestGroupId(gid);
    setChatMobileSurface(gid, "messages");
    setPresentationViewer(null);
    window.setTimeout(() => composerRef.current?.focus(), 0);
  }, [composerRef, groupPresentation, selectedGroupId, setChatMobileSurface, setComposerDestGroupId, setPresentationViewer, setQuotedPresentationRef, showError, t]);

  const handleOpenPresentationWindow = useCallback(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || !splitPresentationViewer) return;
    setChatPresentationDisplayMode(gid, "modal");
    setPresentationViewer({ ...splitPresentationViewer, surface: "modal" });
  }, [selectedGroupId, setChatPresentationDisplayMode, setPresentationViewer, splitPresentationViewer]);

  const handleSplitReplaceSlot = useCallback((slotId: string) => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || !slotId) return;
    setPresentationViewer(null);
    setPresentationPin({ groupId: gid, slotId });
  }, [selectedGroupId, setPresentationPin, setPresentationViewer]);

  const handleSplitClearSlot = useCallback(async (slotId: string) => {
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
  }, [clearPresentationSlotAttention, selectedGroupId, setGroupPresentation, setPresentationPin, setPresentationViewer, showError, t]);

  const handleSplitResizeStart = useCallback((event: React.PointerEvent<HTMLDivElement>) => {
    if (!showDesktopSplitPresentation) return;
    event.preventDefault();
    event.stopPropagation();
    splitResizeRef.current = {
      startX: event.clientX,
      startWidth: effectivePresentationSplitWidth,
    };
    setIsSplitResizing(true);
    document.body.style.setProperty("cursor", "col-resize");
    document.body.style.setProperty("user-select", "none");
  }, [effectivePresentationSplitWidth, showDesktopSplitPresentation]);

  const filterOptions: Array<["all" | "user" | "attention" | "task", string]> = [
    ["all", t('filterAll')],
    ["user", t('filterUser')],
    ["attention", t('filterImportant')],
    ["task", t('filterNeedReply')],
  ];
  const showMessageFilters = !readOnly && !chatWindowProps && hasAnyChatMessages;
  const hasPresentationCards = (groupPresentation?.slots || []).some((slot) => !!slot?.card);
  const showMobilePresentationAction =
    isSmallScreen && !chatWindowProps && !!selectedGroupId && (hasPresentationCards || hasPresentationAttention);
  const showMobileFloatingControls = isSmallScreen && (showMessageFilters || showMobilePresentationAction);
  const mobileMessageTopInsetPx = isSmallScreen ? getMobileMessageTopInsetPx(showMobileFloatingControls) : 0;

  return (
    <div className="flex flex-col h-full w-full overflow-hidden bg-transparent">
      {/* 1. Header Area: For critical banners/setup only, very space-efficient */}
      <header className="flex-shrink-0 z-10 flex flex-col w-full">
        {/* Jump-to window banner */}
        {chatWindowProps && (
          <div className="px-4 pt-4">
            <div
              className={classNames(
                "flex items-center justify-between gap-3 rounded-2xl border px-4 py-3 shadow-sm",
                isDark ? "border-slate-700/50 bg-slate-900/40" : "border-gray-200 bg-white/70"
              )}
              role="status"
              aria-label={t('viewingMessage')}
            >
              <div className="min-w-0">
                <div className={classNames("text-sm font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
                  {t('viewingMessage')}
                </div>
                <div className={classNames("text-xs mt-0.5", isDark ? "text-slate-400" : "text-gray-600")}>
                  {isLoadingHistory
                    ? t('loadingContext')
                    : chatWindowProps.hasMoreBefore || chatWindowProps.hasMoreAfter
                      ? t('contextTruncated')
                      : t('contextLoaded')}
                </div>
              </div>
              {!readOnly && (
                <button
                  type="button"
                  className={classNames(
                    "flex-shrink-0 text-xs font-semibold px-3 py-1.5 min-h-[36px] flex items-center rounded-full border transition-colors",
                    isDark
                      ? "border-slate-600 text-slate-200 hover:bg-slate-800/60"
                      : "border-gray-200 text-gray-800 hover:bg-gray-100"
                  )}
                  onClick={exitChatWindow}
                >
                  {t('returnToLatest')}
                </button>
              )}
            </div>
          </div>
        )}

        {/* Compact setup card */}
        {!readOnly && showSetupCard && chatMessages.length > 0 && (
          <div className="px-4 pt-4 pb-2">
            <div
              className={classNames(
                "rounded-2xl border p-4 sm:p-5",
                isDark ? "border-slate-700/50 bg-slate-900/40" : "border-gray-200 bg-white/70"
              )}
              role="region"
              aria-label={t('setupChecklist')}
            >
              <div className={classNames("text-sm font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
                {t('nextSteps')}
              </div>
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
                  variant="compact"
                />
              </Suspense>
            </div>
          </div>
        )}
      </header>

      {/* 2. Body Area: messages stay primary; presentation is a secondary surface */}
      <main className="flex flex-1 min-h-0 flex-col">
        <div ref={splitLayoutRef} className="relative flex min-h-0 flex-1">
          {(!isSmallScreen || mobileSurface === "messages") ? (
            <section className="relative flex min-h-0 min-w-0 flex-1 flex-col">
              {showMobileFloatingControls && (
                <div
                  className="pointer-events-none absolute inset-x-0 z-30 px-3"
                  style={{ top: MOBILE_FLOATING_CONTROLS_TOP_INSET_PX }}
                >
                  <div className="flex items-center gap-3">
                    {showMessageFilters ? (
                      <div
                        className="pointer-events-auto min-w-0 flex-1 overflow-x-auto scrollbar-hide"
                        role="tablist"
                        aria-label={t('chatFilters')}
                      >
                        <div className={classNames(
                          "inline-flex min-w-max items-center gap-1 rounded-full border p-1 shadow-sm backdrop-blur-xl",
                          isDark
                            ? "border-white/10 bg-black/10"
                            : "border-black/10 bg-white/35"
                        )}>
                          {filterOptions.map(([key, label]) => {
                            const active = chatFilter === key;
                            return (
                              <button
                                key={key}
                                type="button"
                                className={classNames(
                                  "touch-target-sm min-w-0 rounded-full px-3 py-1.5 text-[11px] font-medium transition-all whitespace-nowrap",
                                  active
                                    ? isDark
                                      ? "border border-white/12 bg-white/[0.08] text-white shadow-sm"
                                      : "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] shadow-sm"
                                    : isDark
                                      ? "text-slate-400 hover:text-white hover:bg-white/[0.05]"
                                      : "text-gray-500 hover:text-[rgb(35,36,37)] hover:bg-black/[0.04]"
                                )}
                                onClick={() => setChatFilter(key)}
                                aria-pressed={active}
                              >
                                {label}
                              </button>
                            );
                          })}
                        </div>
                      </div>
                    ) : (
                      <div className="min-w-0 flex-1" aria-hidden="true" />
                    )}

                    {showMobilePresentationAction ? (
                      <button
                        type="button"
                        onClick={() => selectedGroupId && setChatMobileSurface(selectedGroupId, "presentation")}
                        className={classNames(
                          "relative flex h-11 w-11 flex-shrink-0 items-center justify-center rounded-full border backdrop-blur-xl transition-all duration-200",
                          isDark
                            ? "border-white/10 bg-black/10 text-slate-100"
                            : "border-black/10 bg-white/35 text-gray-900 shadow-sm",
                          hasPresentationAttention &&
                            (isDark
                              ? "presentation-slot-attention presentation-slot-attention-dark"
                              : "presentation-slot-attention presentation-slot-attention-light")
                        )}
                        aria-label={t("presentationOpenDockAction", { defaultValue: "Open presentation" })}
                        title={t("presentationOpenDockAction", { defaultValue: "Open presentation" })}
                      >
                        <BookmarkIcon size={18} />
                        {hasPresentationAttention ? (
                          <>
                            <span
                              className={classNames(
                                "pointer-events-none absolute right-2 top-2 h-2 w-2 rounded-full animate-ping",
                                isDark ? "bg-cyan-300/70" : "bg-cyan-500/45"
                              )}
                            />
                            <span
                              className={classNames(
                                "pointer-events-none absolute right-2 top-2 h-2 w-2 rounded-full",
                                isDark ? "bg-cyan-200" : "bg-cyan-500"
                              )}
                            />
                          </>
                        ) : null}
                      </button>
                    ) : null}
                  </div>
                </div>
              )}

              {showMessageFilters && (
                <div
                  className="hidden md:block absolute top-4 left-4 z-20 pointer-events-none"
                  style={{ width: "calc(100% - 32px)" }}
                >
                  <div
                    className={classNames(
                      "inline-flex items-center gap-1 xl:gap-2 rounded-full border p-1 sm:p-1.5 shadow-xl pointer-events-auto backdrop-blur-xl transition-all duration-300",
                      isDark
                        ? "border-white/10 bg-slate-900/60 shadow-black/40 ring-1 ring-white/5"
                        : "border-black/5 bg-white/70 shadow-gray-200/50 ring-1 ring-black/5"
                    )}
                    role="tablist"
                    aria-label={t('chatFilters')}
                  >
                    {filterOptions.map(([key, label]) => {
                      const active = chatFilter === key;
                      return (
                        <button
                          key={key}
                          type="button"
                          className={classNames(
                            "text-xs px-4 py-1.5 rounded-full transition-all font-medium",
                            active
                              ? isDark
                                ? "border border-white/12 bg-white/[0.08] text-white shadow-sm"
                                : "border border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] shadow-sm"
                              : isDark
                                ? "text-slate-400 hover:text-white hover:bg-white/[0.05]"
                                : "text-gray-500 hover:text-[rgb(35,36,37)] hover:bg-black/[0.04]"
                          )}
                          onClick={() => setChatFilter(key)}
                          aria-pressed={active}
                        >
                          {label}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}

              {isBusinessEmptyState && showSetupCard ? (
                <div
                  ref={scrollRef}
                  className="flex-1 min-h-0 overflow-auto px-4 py-4 relative"
                  role="log"
                  aria-label={t('chatMessages')}
                >
                  <div className="flex h-full flex-col items-center justify-center text-center pb-20">
                    <div className={classNames("w-full max-w-md", isDark ? "text-slate-200" : "text-gray-800")}>
                      <div className="mb-4 flex justify-center" aria-hidden="true">
                        <CompassIcon size={32} className={isDark ? "text-white" : "text-[rgb(35,36,37)]"} />
                      </div>
                      <div className={classNames("text-sm font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
                        {t('nextSteps')}
                      </div>
                      {readOnly ? (
                        <div className={classNames("mt-3 text-sm", isDark ? "text-slate-400" : "text-gray-600")}>
                          {t('noMessagesYet')}
                        </div>
                      ) : (
                        <Suspense fallback={<ChatLazyFallback className="mt-4" />}>
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
                <VirtualMessageList
                  messages={chatMessages}
                  actors={actors}
                  agentStates={agentStates}
                  taskById={taskById}
                  isDark={isDark}
                  readOnly={readOnly}
                  groupId={selectedGroupId}
                  groupLabelById={groupLabelById}
                  viewKey={chatViewKey}
                  initialScrollTargetId={chatInitialScrollTargetId}
                  initialScrollAnchorId={chatInitialScrollAnchorId}
                  initialScrollAnchorOffsetPx={chatInitialScrollAnchorOffsetPx}
                  highlightEventId={chatHighlightEventId}
                  scrollRef={scrollRef}
                  topInsetPx={mobileMessageTopInsetPx}
                  onReply={startReply}
                  onShowRecipients={showRecipients}
                  onCopyLink={copyMessageLink}
                  onCopyContent={copyMessageText}
                  onRelay={relayMessage}
                  onOpenSource={openSourceMessage}
                  onOpenPresentationRef={openPresentationRef}
                  onOpenTaskRef={openTaskRef}
                  showScrollButton={showScrollButton}
                  onScrollButtonClick={handleScrollButtonClick}
                  chatUnreadCount={chatUnreadCount}
                  forceStickToBottomToken={forceStickToBottomToken}
                  onScrollChange={handleScrollChange}
                  onScrollSnapshot={handleScrollSnapshot}
                  isLoadingHistory={listIsLoadingHistory}
                  hasMoreHistory={listHasMoreHistory}
                  onLoadMore={loadMoreHistory}
                />
              )}

              {!chatWindowProps && runtimeActors.length > 0 ? (
                <div className="pointer-events-none absolute inset-x-0 bottom-1 z-20 sm:bottom-2">
                  <RuntimeDock
                    groupId={selectedGroupId}
                    runtimeActors={runtimeActors}
                    liveWorkCards={liveWorkCards}
                    activeRuntimeActorId={activeRuntimeActorId}
                    isDark={isDark}
                    isSmallScreen={isSmallScreen}
                    readOnly={readOnly}
                    selectedGroupRunning={selectedGroupRunning}
                    selectedGroupActorsHydrating={selectedGroupActorsHydrating}
                    onAddAgent={!readOnly ? addAgent : undefined}
                    onOpenRuntimeActor={onOpenRuntimeActor}
                  />
                </div>
              ) : null}
            </section>
          ) : null}

          {showDesktopSplitPresentation ? (
            <>
              <div
                className="relative hidden w-2 flex-shrink-0 cursor-col-resize md:block"
                onPointerDown={handleSplitResizeStart}
                aria-hidden="true"
              >
                <div
                  className={classNames(
                    "absolute inset-y-0 left-1/2 w-px -translate-x-1/2",
                    isDark ? "bg-white/8" : "bg-black/8"
                  )}
                />
                <div
                  className={classNames(
                    "absolute inset-y-0 -left-1 w-4 rounded-full transition-colors",
                    isSplitResizing
                      ? isDark
                        ? "bg-cyan-300/18"
                        : "bg-cyan-500/16"
                      : isDark
                        ? "hover:bg-white/8"
                        : "hover:bg-black/6"
                  )}
                />
              </div>
              <div
                className={classNames(
                  "hidden min-h-0 flex-shrink-0 overflow-hidden border-l md:flex",
                  isDark ? "border-white/8 bg-slate-950/20" : "border-black/8 bg-white/40"
                )}
                style={{ width: `${effectivePresentationSplitWidth}px` }}
              >
                <Suspense fallback={<ChatLazyFallback className="w-[52px]" />}>
                  <PresentationRail
                    mode="split"
                    presentation={groupPresentation}
                    isDark={isDark}
                    readOnly={readOnly}
                    attentionSlots={presentationAttention}
                    onOpenSlot={openPresentationSlot}
                    onPinSlot={pinPresentationSlot}
                  />
                </Suspense>
                <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                  <PresentationViewerSplitPanel
                    isDark={isDark}
                    readOnly={readOnly}
                    groupId={selectedGroupId}
                    slotId={splitPresentationViewer.slotId}
                    presentation={groupPresentation}
                    focusRef={splitPresentationViewer.focusRef || null}
                    focusEventId={splitPresentationViewer.focusEventId || null}
                    onQuoteInChat={handleQuotePresentationReference}
                    onReplaceSlot={handleSplitReplaceSlot}
                    onClearSlot={(slotId) => { void handleSplitClearSlot(slotId); }}
                    onOpenWindow={handleOpenPresentationWindow}
                    onClose={() => setPresentationViewer(null)}
                  />
                </Suspense>
              </div>
            </>
          ) : !isSmallScreen ? (
            <Suspense fallback={<ChatLazyFallback className="w-0" />}>
              <PresentationRail
                mode="dock"
                presentation={groupPresentation}
                isDark={isDark}
                readOnly={readOnly}
                isOpen={presentationDockOpen}
                onOpenChange={setPresentationDockOpen}
                attentionSlots={presentationAttention}
                onOpenSlot={openPresentationSlot}
                onPinSlot={pinPresentationSlot}
              />
            </Suspense>
          ) : null}

          {isSmallScreen && mobileSurface === "presentation" ? (
            <div className="flex min-h-0 flex-1 flex-col">
              <Suspense fallback={<ChatLazyFallback className="flex-1" />}>
                <PresentationRail
                  mode="panel"
                  presentation={groupPresentation}
                  isDark={isDark}
                  readOnly={readOnly}
                  isOpen={mobileSurface === "presentation"}
                  onOpenChange={(open) => selectedGroupId && setChatMobileSurface(selectedGroupId, open ? "presentation" : "messages")}
                  attentionSlots={presentationAttention}
                  onOpenSlot={openPresentationSlot}
                  onPinSlot={pinPresentationSlot}
                />
              </Suspense>
            </div>
          ) : null}
        </div>
      </main>

      {/* 3. Footer Area: Composer */}
      {!readOnly && (
        <footer className="flex-shrink-0 w-full bg-transparent">
          <ChatComposer
            isDark={isDark}
            isSmallScreen={isSmallScreen}
            selectedGroupId={selectedGroupId}
            actors={actors}
            recipientActors={recipientActors}
            recipientActorsBusy={recipientActorsBusy}
            groups={groups}
            destGroupId={destGroupId}
            setDestGroupId={setDestGroupId}
            destGroupScopeLabel={destGroupScopeLabel}
            busy={busy}
            recentMessages={chatMessages}
            replyTarget={replyTarget}
            onCancelReply={cancelReply}
            quotedPresentationRef={quotedPresentationRef}
            onClearQuotedPresentationRef={clearQuotedPresentationRef}
            toTokens={toTokens}
            onToggleRecipient={toggleRecipient}
            onClearRecipients={clearRecipients}
            composerFiles={composerFiles}
            onRemoveComposerFile={removeComposerFile}
            appendComposerFiles={appendComposerFiles}
            fileInputRef={fileInputRef}
            composerRef={composerRef}
            composerText={composerText}
            setComposerText={setComposerText}
            priority={priority}
            replyRequired={replyRequired}
            setPriority={setPriority}
            setReplyRequired={setReplyRequired}
            onSendMessage={sendMessage}
            showMentionMenu={showMentionMenu}
            setShowMentionMenu={setShowMentionMenu}
            mentionSuggestions={mentionSuggestions}
            mentionSelectedIndex={mentionSelectedIndex}
            setMentionSelectedIndex={setMentionSelectedIndex}
            setMentionFilter={setMentionFilter}
            onAppendRecipientToken={appendRecipientToken}
          />
        </footer>
      )}
    </div>
  );
}
