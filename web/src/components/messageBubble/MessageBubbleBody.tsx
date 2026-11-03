import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type {
  LedgerEvent,
  PresentationMessageRef,
  Task,
  TaskMessageRef,
  VoiceDocumentMessageRef,
} from "../../types";
import { classNames } from "../../utils/classNames";
import { MessageAttachments } from "./MessageAttachments";
import { MessageReferenceSections } from "./MessageReferenceSections";
import { MessageReplyQuote } from "./MessageReplyQuote";

export function MessageBubbleBody({
  event,
  messageContent,
  isUserMessage,
  isDark,
  quoteText,
  replyToEventId,
  presentationRefs,
  voiceDocumentRefs,
  taskRefs,
  taskById,
  insight,
  blobAttachments,
  blobGroupId,
  stableMessageAttachmentKey,
  onOpenPresentationRef,
  onOpenTaskRef,
  onOpenReplyTarget,
}: {
  event: LedgerEvent;
  messageContent: ReactNode;
  isUserMessage: boolean;
  isDark: boolean;
  quoteText?: string;
  replyToEventId?: string;
  presentationRefs: PresentationMessageRef[];
  voiceDocumentRefs: VoiceDocumentMessageRef[];
  taskRefs: TaskMessageRef[];
  taskById: Map<string, Task>;
  insight: string;
  blobAttachments: Array<{
    kind: string;
    path: string;
    title: string;
    bytes: number;
    mime_type: string;
    local_preview_url: string;
  }>;
  blobGroupId: string;
  stableMessageAttachmentKey: string;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  onOpenReplyTarget?: (replyToEventId: string) => void;
}) {
  const { t } = useTranslation("chat");
  const supportingSectionClass = classNames(
    "mt-3 border-t pt-3",
    "border-[var(--glass-border-subtle)]",
  );
  return (
    <>
      {quoteText && (
        <MessageReplyQuote
          key={replyToEventId || quoteText}
          text={quoteText}
          replyToEventId={replyToEventId}
          onOpenReplyTarget={onOpenReplyTarget}
        />
      )}

      <MessageReferenceSections
        event={event}
        presentationRefs={presentationRefs}
        voiceDocumentRefs={voiceDocumentRefs}
        taskRefs={taskRefs}
        taskById={taskById}
        sectionClassName={supportingSectionClass}
        onOpenPresentationRef={onOpenPresentationRef}
        onOpenTaskRef={onOpenTaskRef}
      />

      {messageContent}

      {/* Attachments are part of the message: they follow its text with no rule between
          them, and the sender's insight about the whole message follows them. */}
      <MessageAttachments
        attachments={blobAttachments}
        blobGroupId={blobGroupId}
        isUserMessage={isUserMessage}
        isDark={isDark}
        attachmentKeyPrefix={stableMessageAttachmentKey}
        downloadTitle={(name) => t("download", { name })}
      />

      {insight ? (
        <div className={supportingSectionClass} data-message-insight>
          <div className="mb-1.5 text-xs font-semibold uppercase text-[var(--color-text-tertiary)]">
            {t("senderPerspective")}
          </div>
          <div className="max-w-full break-words whitespace-pre-wrap text-[var(--color-text-secondary)] [overflow-wrap:anywhere]">
            {insight}
          </div>
        </div>
      ) : null}
    </>
  );
}
