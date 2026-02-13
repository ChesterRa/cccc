// ChatTab is the main chat page component.
// Refactored to use useChatTab hook for business logic, reducing prop drilling.

import type { MutableRefObject, RefObject } from "react";
import { Actor, GroupMeta } from "../../types";
import { VirtualMessageList } from "../../components/VirtualMessageList";
import { classNames } from "../../utils/classNames";
import { SetupChecklist } from "./SetupChecklist";
import { ChatComposer } from "./ChatComposer";
import { useChatTab } from "../../hooks/useChatTab";
import { useTranslation } from 'react-i18next';

export interface ChatTabProps {
  // UI configuration
  isDark: boolean;
  isSmallScreen: boolean;
  readOnly?: boolean;

  // Core data (must be passed from App)
  selectedGroupId: string;
  groupLabelById: Record<string, string>;
  actors: Actor[];
  groups: GroupMeta[];

  // Recipient actors for cross-group messaging
  recipientActors: Actor[];
  recipientActorsBusy?: boolean;
  destGroupScopeLabel?: string;

  // Refs (shared with App for external interactions)
  scrollRef: MutableRefObject<HTMLDivElement | null>;
  composerRef: RefObject<HTMLTextAreaElement>;
  fileInputRef: RefObject<HTMLInputElement>;

  // Refs for scroll state (shared with App)
  chatAtBottomRef?: MutableRefObject<boolean>;
  chatScrollMemoryRef?: MutableRefObject<
    Record<string, { atBottom: boolean; anchorId: string; offsetPx: number }>
  >;

  // Scroll restore (computed in App)
  chatInitialScrollAnchorId?: string;
  chatInitialScrollAnchorOffsetPx?: number;

  // File handling (from useDragDrop)
  appendComposerFiles: (files: File[]) => void;

  // Group actions (from useGroupActions)
  onStartGroup: () => void;

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
  groupLabelById,
  actors,
  groups,
  recipientActors,
  recipientActorsBusy,
  destGroupScopeLabel,
  scrollRef,
  composerRef,
  fileInputRef,
  chatAtBottomRef,
  chatScrollMemoryRef,
  chatInitialScrollAnchorId,
  chatInitialScrollAnchorOffsetPx,
  appendComposerFiles,
  onStartGroup,
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
    hasAnyChatMessages,
    chatFilter,
    setChatFilter,
    chatViewKey,
    chatWindowProps,
    chatInitialScrollTargetId,
    chatHighlightEventId,
    isLoadingHistory,
    hasMoreHistory,
    loadMoreHistory,

    // UI state
    busy,
    showScrollButton,
    chatUnreadCount,

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
    cancelReply,
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

    // Presence
    presenceAgents,

    // Actions
    sendMessage,
    acknowledgeMessage,
    copyMessageLink,
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
    actors,
    recipientActors,
    composerRef,
    fileInputRef,
    chatAtBottomRef,
    chatScrollMemoryRef,
  });

  const { t } = useTranslation('chat');

  // Empty state: show full-screen setup guidance.
  if (chatMessages.length === 0 && showSetupCard) {
    return (
      <>
        <div
          ref={scrollRef}
          className="flex-1 min-h-0 overflow-auto px-4 py-4 relative"
          role="log"
          aria-label={t('chatMessages')}
        >
          <div className="flex flex-col items-center justify-center h-full text-center pb-20">
            <div className={classNames("w-full max-w-md", isDark ? "text-slate-200" : "text-gray-800")}>
              <div className="text-4xl mb-4">&#x1F9ED;</div>
              <div className={classNames("text-sm font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
                {t('nextSteps')}
              </div>
              {readOnly ? (
                <div className={classNames("mt-3 text-sm", isDark ? "text-slate-400" : "text-gray-600")}>
                  {t('noMessagesYet')}
                </div>
              ) : (
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
              )}
            </div>
          </div>
        </div>
        {!readOnly && (
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
            replyTarget={replyTarget}
            onCancelReply={cancelReply}
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
        )}
      </>
    );
  }

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
            </div>
          </div>
        )}
      </header>

      {/* 2. Body Area: Contains the List + the Floating Filter Pill */}
      <main className="flex-1 min-h-0 relative flex flex-col">
        {/* Chat Filter Pill — inline on mobile, floating on sm+ */}
        {!readOnly && !chatWindowProps && hasAnyChatMessages && (
          <>
            {/* Mobile: inline scrollable bar */}
            <div
              className={classNames(
                "sm:hidden flex-shrink-0 overflow-x-auto scrollbar-hide px-3 py-2 border-b",
                isDark ? "border-white/5" : "border-black/5"
              )}
              role="tablist"
              aria-label={t('chatFilters')}
            >
              <div className={classNames(
                "inline-flex items-center gap-1 rounded-full border p-1 backdrop-blur-md",
                isDark
                  ? "border-slate-700/60 bg-slate-900/60"
                  : "border-gray-200/80 bg-white/70"
              )}>
                {[
                  ["all", t('filterAll')],
                  ["to_user", t('filterToUser')],
                  ["attention", t('filterImportant')],
                  ["task", t('filterNeedReply')],
                ].map(([key, label]) => {
                  const k = key as "all" | "to_user" | "attention" | "task";
                  const active = chatFilter === k;
                  return (
                    <button
                      key={k}
                      type="button"
                      className={classNames(
                        "text-xs px-3 py-1.5 rounded-full transition-all font-medium whitespace-nowrap flex-shrink-0",
                        active
                          ? "bg-blue-600 text-white shadow-sm"
                          : isDark
                            ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800/60"
                            : "text-gray-500 hover:text-gray-900 hover:bg-gray-100"
                      )}
                      onClick={() => setChatFilter(k)}
                      aria-pressed={active}
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            </div>
            {/* Desktop: floating pill over message list */}
            <div
              className="hidden sm:block absolute top-4 left-4 z-20 pointer-events-none"
              style={{ width: "calc(100% - 32px)" }}
            >
              <div
                className={classNames(
                  "inline-flex items-center gap-1 rounded-full border p-1 shadow-lg pointer-events-auto backdrop-blur-md transition-all",
                  isDark
                    ? "border-slate-700/60 bg-slate-900/60 shadow-black/20"
                    : "border-gray-200/80 bg-white/70 shadow-gray-200/50"
                )}
                role="tablist"
                aria-label={t('chatFilters')}
              >
                {[
                  ["all", t('filterAll')],
                  ["to_user", t('filterToUser')],
                  ["attention", t('filterImportant')],
                  ["task", t('filterNeedReply')],
                ].map(([key, label]) => {
                  const k = key as "all" | "to_user" | "attention" | "task";
                  const active = chatFilter === k;
                  return (
                    <button
                      key={k}
                      type="button"
                      className={classNames(
                        "text-xs px-4 py-1.5 rounded-full transition-all font-medium",
                        active
                          ? "bg-blue-600 text-white shadow-sm"
                          : isDark
                            ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800/60"
                            : "text-gray-500 hover:text-gray-900 hover:bg-gray-100"
                      )}
                      onClick={() => setChatFilter(k)}
                      aria-pressed={active}
                    >
                      {label}
                    </button>
                  );
                })}
              </div>
            </div>
          </>
        )}

        <VirtualMessageList
          messages={chatMessages}
          actors={actors}
          presenceAgents={presenceAgents}
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
          onReply={startReply}
          onShowRecipients={showRecipients}
          onAck={acknowledgeMessage}
          onCopyLink={copyMessageLink}
          onRelay={relayMessage}
          onOpenSource={openSourceMessage}
          showScrollButton={showScrollButton}
          onScrollButtonClick={handleScrollButtonClick}
          chatUnreadCount={chatUnreadCount}
          onScrollChange={handleScrollChange}
          onScrollSnapshot={handleScrollSnapshot}
          isLoadingHistory={isLoadingHistory}
          hasMoreHistory={hasMoreHistory}
          onLoadMore={loadMoreHistory}
        />
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
            replyTarget={replyTarget}
            onCancelReply={cancelReply}
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
