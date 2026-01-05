// ChatTab - 聊天页面主组件
import type { Dispatch, MutableRefObject, RefObject, SetStateAction } from "react";
import { Actor, LedgerEvent, ReplyTarget } from "../../types";
import { VirtualMessageList } from "../../components/VirtualMessageList";
import { classNames } from "../../utils/classNames";
import { SetupChecklist } from "./SetupChecklist";
import { ChatComposer } from "./ChatComposer";

export interface ChatTabProps {
  isDark: boolean;
  isSmallScreen: boolean;
  selectedGroupId: string;
  actors: Actor[];
  busy: string;

  // Setup checklist
  showSetupCard: boolean;
  needsScope: boolean;
  needsActors: boolean;
  needsStart: boolean;
  hasForeman: boolean;
  onAddAgent: () => void;
  onStartGroup: () => void;

  // Messages + scrolling
  chatMessages: LedgerEvent[];
  scrollRef: MutableRefObject<HTMLDivElement | null>;
  showScrollButton: boolean;
  chatUnreadCount: number;
  onScrollButtonClick: () => void;
  onScrollChange: (isAtBottom: boolean) => void;
  onReply: (ev: LedgerEvent) => void;
  onShowRecipients: (eventId: string) => void;
  initialScrollTop?: number; // 用于恢复滚动位置

  // Composer
  replyTarget: ReplyTarget;
  onCancelReply: () => void;
  toTokens: string[];
  onToggleRecipient: (token: string) => void;
  onClearRecipients: () => void;

  composerFiles: File[];
  onRemoveComposerFile: (index: number) => void;
  appendComposerFiles: (files: File[]) => void;

  fileInputRef: RefObject<HTMLInputElement>;
  composerRef: RefObject<HTMLTextAreaElement>;
  composerText: string;
  setComposerText: Dispatch<SetStateAction<string>>;
  onSendMessage: () => void;

  // Mention menu
  showMentionMenu: boolean;
  setShowMentionMenu: Dispatch<SetStateAction<boolean>>;
  mentionSuggestions: string[];
  mentionSelectedIndex: number;
  setMentionSelectedIndex: Dispatch<SetStateAction<number>>;
  setMentionFilter: Dispatch<SetStateAction<string>>;
  onAppendRecipientToken: (token: string) => void;
}

export function ChatTab({
  isDark,
  isSmallScreen,
  selectedGroupId,
  actors,
  busy,
  showSetupCard,
  needsScope,
  needsActors,
  needsStart,
  onAddAgent,
  onStartGroup,
  chatMessages,
  scrollRef,
  showScrollButton,
  chatUnreadCount,
  onScrollButtonClick,
  onScrollChange,
  onReply,
  onShowRecipients,
  initialScrollTop,
  replyTarget,
  onCancelReply,
  toTokens,
  onToggleRecipient,
  onClearRecipients,
  composerFiles,
  onRemoveComposerFile,
  appendComposerFiles,
  fileInputRef,
  composerRef,
  composerText,
  setComposerText,
  onSendMessage,
  showMentionMenu,
  setShowMentionMenu,
  mentionSuggestions,
  mentionSelectedIndex,
  setMentionSelectedIndex,
  setMentionFilter,
  onAppendRecipientToken,
}: ChatTabProps) {
  // 空状态 - 显示全屏设置引导
  if (chatMessages.length === 0 && showSetupCard) {
    return (
      <>
        <div
          ref={scrollRef}
          className="flex-1 min-h-0 overflow-auto px-4 py-4 relative"
          role="log"
          aria-label="Chat messages"
        >
          <div className="flex flex-col items-center justify-center h-full text-center pb-20">
            <div className={classNames("w-full max-w-md", isDark ? "text-slate-200" : "text-gray-800")}>
              <div className="text-4xl mb-4">🧭</div>
              <div className={classNames("text-sm font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
                Next steps
              </div>
              <SetupChecklist
                isDark={isDark}
                selectedGroupId={selectedGroupId}
                busy={busy}
                needsScope={needsScope}
                needsActors={needsActors}
                needsStart={needsStart}
                onAddAgent={onAddAgent}
                onStartGroup={onStartGroup}
                variant="full"
              />
            </div>
          </div>
        </div>
        <ChatComposer
          isDark={isDark}
          isSmallScreen={isSmallScreen}
          selectedGroupId={selectedGroupId}
          actors={actors}
          busy={busy}
          replyTarget={replyTarget}
          onCancelReply={onCancelReply}
          toTokens={toTokens}
          onToggleRecipient={onToggleRecipient}
          onClearRecipients={onClearRecipients}
          composerFiles={composerFiles}
          onRemoveComposerFile={onRemoveComposerFile}
          appendComposerFiles={appendComposerFiles}
          fileInputRef={fileInputRef}
          composerRef={composerRef}
          composerText={composerText}
          setComposerText={setComposerText}
          onSendMessage={onSendMessage}
          showMentionMenu={showMentionMenu}
          setShowMentionMenu={setShowMentionMenu}
          mentionSuggestions={mentionSuggestions}
          mentionSelectedIndex={mentionSelectedIndex}
          setMentionSelectedIndex={setMentionSelectedIndex}
          setMentionFilter={setMentionFilter}
          onAppendRecipientToken={onAppendRecipientToken}
        />
      </>
    );
  }

  return (
    <>
      {/* 紧凑设置卡片 - 有消息时显示在顶部 */}
      {showSetupCard && chatMessages.length > 0 && (
        <div className="flex-shrink-0 px-4 pt-4">
          <div
            className={classNames(
              "rounded-2xl border p-4 sm:p-5",
              isDark ? "border-slate-700/50 bg-slate-900/40" : "border-gray-200 bg-white/70"
            )}
            role="region"
            aria-label="Setup checklist"
          >
            <div className={classNames("text-sm font-semibold", isDark ? "text-slate-200" : "text-gray-800")}>
              Next steps
            </div>
            <SetupChecklist
              isDark={isDark}
              selectedGroupId={selectedGroupId}
              busy={busy}
              needsScope={needsScope}
              needsActors={needsActors}
              needsStart={needsStart}
              onAddAgent={onAddAgent}
              onStartGroup={onStartGroup}
              variant="compact"
            />
          </div>
        </div>
      )}

      {/* 消息列表 */}
      <VirtualMessageList
        messages={chatMessages}
        actors={actors}
        isDark={isDark}
        groupId={selectedGroupId || ""}
        scrollRef={scrollRef}
        onReply={onReply}
        onShowRecipients={onShowRecipients}
        showScrollButton={showScrollButton}
        onScrollButtonClick={onScrollButtonClick}
        chatUnreadCount={chatUnreadCount}
        onScrollChange={onScrollChange}
        initialScrollTop={initialScrollTop}
      />

      {/* 消息输入 */}
      <ChatComposer
        isDark={isDark}
        isSmallScreen={isSmallScreen}
        selectedGroupId={selectedGroupId}
        actors={actors}
        busy={busy}
        replyTarget={replyTarget}
        onCancelReply={onCancelReply}
        toTokens={toTokens}
        onToggleRecipient={onToggleRecipient}
        onClearRecipients={onClearRecipients}
        composerFiles={composerFiles}
        onRemoveComposerFile={onRemoveComposerFile}
        appendComposerFiles={appendComposerFiles}
        fileInputRef={fileInputRef}
        composerRef={composerRef}
        composerText={composerText}
        setComposerText={setComposerText}
        onSendMessage={onSendMessage}
        showMentionMenu={showMentionMenu}
        setShowMentionMenu={setShowMentionMenu}
        mentionSuggestions={mentionSuggestions}
        mentionSelectedIndex={mentionSelectedIndex}
        setMentionSelectedIndex={setMentionSelectedIndex}
        setMentionFilter={setMentionFilter}
        onAppendRecipientToken={onAppendRecipientToken}
      />
    </>
  );
}
