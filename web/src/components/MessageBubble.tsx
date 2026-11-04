import { memo, useCallback, useContext, useMemo, useState } from "react";
import { FloatingPortal, autoUpdate, flip, offset, shift, useFloating } from "@floating-ui/react";
import { useTranslation } from "react-i18next";
import { useCopyFeedback } from "../hooks/useCopyFeedback";
import {
  LedgerEvent,
  Actor,
  AgentState,
  Task,
  getActorAccentIndex,
  ChatMessageData,
  MessageAttachment,
  PresentationMessageRef,
  TaskMessageRef,
} from "../types";
import { formatFullTime, formatMessageTimestamp } from "../utils/time";
import { classNames } from "../utils/classNames";
import { buildReplyComposerState, getReplyEventId } from "../utils/chatReply";
import { projectCrossGroupRecipients, projectMessageMode } from "../utils/crossGroupRecipients";
import { connectGroupLabel, isCrossInstanceInboundMessage } from "../utils/crossInstanceMessages";
import { getPresentationMessageRefs } from "../utils/presentationRefs";
import { getVoiceDocumentMessageRefs } from "../utils/voiceDocumentRefs";
import { getTaskMessageRefs } from "../utils/taskRefs";
import { isRedundantWecomImagePlaceholder } from "../utils/messageAttachments";
import { getMessageInsight } from "../utils/messagePerspective";
import { getMessageMentionTokens } from "../utils/mentionHighlight";
import { getDelegationDisplayText, isDelegationSourceOutbound } from "./messageBubbleDelegation";
import { MessageActions } from "./messageBubble/MessageActions";
import {
  QUICK_REPLY_EMOJI,
  QUICK_REPLY_KINDS,
  sendQuickReply,
  type QuickReplyKind,
  type QuickReplyTarget,
} from "./messageBubble/quickReplies";
import { MessageFooter } from "./messageBubble/MessageBubbleChrome";
import { withAuthToken } from "../services/api/base";
import type { WebModelDeliveryStatus } from "../utils/webModelDeliveryStatus";
import {
  buildToLabel,
  buildVisibleReadStatusEntries,
  computeObligationSummary,
  getSenderDisplayName,
} from "./messageBubble/model";
import { ActorAvatar } from "./ActorAvatar";
import { formatEventLine, mayContainMarkdown } from "./messageBubble/helpers";
import { AgentStateTooltip } from "./messageBubble/AgentStateTooltip";
import { MessageContent } from "./messageBubble/MessageContent";
import { MessageBubbleBody } from "./messageBubble/MessageBubbleBody";
import { buildMessageCopyText } from "./messageBubble/messageCopyText";
import { MessageRoutingMetadata } from "./messageBubble/MessageRoutingMetadata";
import { PendingReplySlot } from "./messageBubble/PendingReplySlot";
import { PendingReplyStoreContext } from "../features/pendingReplies/store";
import { useMessagePinned } from "../features/pins/pinStore";
import { PinIcon } from "./Icons";
import { useGroupStore, useUIStore } from "../stores";
import { CHET_NAME, CHET_PRINCIPAL, chetGeneration, isChetMessage } from "../utils/chetIdentity";
import {
  localizedAnswerReceipt,
  pendingReplyView,
  recordedQuestionText,
} from "../features/pendingReplies/model";
import { LivePendingStatusContext } from "../features/pendingReplies/liveStatusContext";

function buildSenderAvatarUrl(groupId: string, senderAvatarPath?: string): string {
  const gid = String(groupId || "").trim();
  const relPath = String(senderAvatarPath || "").trim();
  if (!gid || !relPath.startsWith("state/blobs/")) return "";
  const blobName = relPath.split("/").pop() || "";
  if (!blobName) return "";
  return withAuthToken(
    `/api/v1/groups/${encodeURIComponent(gid)}/blobs/${encodeURIComponent(blobName)}`,
  );
}

export interface MessageBubbleProps {
  event: LedgerEvent;
  actorById: Map<string, Actor>;
  actors: Actor[];
  displayNameMap: Map<string, string>;
  agentState: AgentState | null;
  taskById: Map<string, Task>;
  isDark: boolean;
  readOnly?: boolean;
  groupId: string;
  groupLabelById: Record<string, string>;
  webModelDeliveryStatus?: WebModelDeliveryStatus;
  isHighlighted?: boolean;
  collapseHeader?: boolean;
  /** The message before this one is from the same sender. */
  sameSender?: boolean;
  resolvedReplyQuoteText?: string;
  onReply: () => void;
  onShowRecipients: () => void;
  onCopyLink?: (eventId: string) => void;
  /** Pin or unpin this message; absent where pins are not offered. */
  onTogglePin?: (eventId: string, pinned: boolean) => void;
  onViewContext?: (eventId: string) => void;
  onRelay?: (ev: LedgerEvent) => void;
  onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
  onOpenReplyTarget?: (replyToEventId: string) => void;
}

export const MessageBubble = memo(
  function MessageBubble({
    event: ev,
    actorById,
    actors,
    displayNameMap,
    agentState,
    taskById,
    isDark,
    readOnly,
    groupId,
    groupLabelById,
    webModelDeliveryStatus,
    isHighlighted,
    collapseHeader,
    sameSender,
    resolvedReplyQuoteText,
    onReply,
    onShowRecipients,
    onCopyLink,
    onTogglePin,
    onViewContext,
    onRelay,
    onOpenSource,
    onOpenPresentationRef,
    onOpenTaskRef,
    onOpenReplyTarget,
  }: MessageBubbleProps) {
    const isUserMessage = ev.by === "user";
    const isOptimistic = !!(ev.data as Record<string, unknown> | undefined)?._optimistic;
    const isStreaming = !!ev._streaming;
    // A filtered view's way into the whole conversation; a message still being sent or
    // written has no place there yet.
    const showInConversation =
      onViewContext &&
      !readOnly &&
      typeof ev.id === "string" &&
      ev.id &&
      !isOptimistic &&
      !isStreaming
        ? onViewContext
        : undefined;
    const canReply = !!getReplyEventId(ev);
    // Only a message already in the ledger can be pinned.
    const pinnableId =
      !readOnly && typeof ev.id === "string" && ev.id && !isOptimistic && !isStreaming
        ? ev.id
        : undefined;
    const pinned = useMessagePinned(groupId, pinnableId);
    const messageText = useMemo(() => formatEventLine(ev), [ev]);
    const mentionTokens = useMemo(
      () => getMessageMentionTokens(actors, ev.data),
      [actors, ev.data],
    );
    const pendingStore = useContext(PendingReplyStoreContext);
    const liveStatuses = useContext(LivePendingStatusContext);
    const liveEntry = ev.id && liveStatuses ? liveStatuses[ev.id] : undefined;
    const liveStatus = liveEntry?.status;
    const replyEvent = useMemo(
      () =>
        liveEntry === undefined
          ? ev
          : {
              ...ev,
              _user_request: liveStatus?.user_request,
              _obligation_status: liveStatus?.obligation_status,
            },
      [ev, liveEntry, liveStatus],
    );
    // Whether this is a request to the user follows from the event alone, so it
    // holds while the reply store is not available yet.
    const userRequest = readOnly ? null : pendingReplyView(replyEvent);
    const replyView = pendingStore ? userRequest : null;
    const showPendingPrompts = useUIStore((state) => state.showPendingPrompts);

    const [isAgentStateOpen, setIsAgentStateOpen] = useState(false);
    const floatingMiddleware = useMemo(() => [offset(8), flip(), shift({ padding: 8 })], []);
    const { refs, floatingStyles, context } = useFloating({
      open: isAgentStateOpen,
      onOpenChange: setIsAgentStateOpen,
      placement: "bottom-start",
      middleware: floatingMiddleware,
      whileElementsMounted: autoUpdate,
      strategy: "fixed",
    });
    const isAgentStatePositioned = context.isPositioned;
    const setAgentStateReference = useCallback(
      (node: HTMLElement | null) => {
        refs.setReference(node);
      },
      [refs],
    );
    const setAgentStateFloating = useCallback(
      (node: HTMLElement | null) => {
        refs.setFloating(node);
      },
      [refs],
    );

    const canShowAgentState = useMemo(() => {
      if (isUserMessage) return false;
      const id = String(ev.by || "");
      // Chet is the instance's service, not a member: it has no state in this Group.
      if (!id || id === CHET_PRINCIPAL) return false;
      return true;
    }, [ev.by, isUserMessage]);

    const { t } = useTranslation("chat");
    const copyWithFeedback = useCopyFeedback();

    const agentStateText = String(agentState?.hot?.focus || "").trim();
    const agentStateDisplay = agentStateText || t("noAgentStateYet");
    const stateTask = String(agentState?.hot?.active_task_id || "").trim();
    const stateNext = String(agentState?.hot?.next_action || "").trim();
    const stateChanged = String(agentState?.warm?.what_changed || "").trim();
    const blockerCount = Array.isArray(agentState?.hot?.blockers)
      ? agentState.hot.blockers.length
      : 0;

    // Treat data as ChatMessageData.
    const msgData = ev.data as ChatMessageData | undefined;
    const insight = getMessageInsight(msgData);
    const quoteText =
      String(msgData?.quote_text || resolvedReplyQuoteText || "").trim() || undefined;
    const replyToEventId =
      typeof msgData?.reply_to === "string" ? String(msgData.reply_to || "").trim() : "";
    // Chet's name shows only on a message this Group, as it is now, proves Chet wrote.
    const groupGeneration = useGroupStore((state) =>
      state.groupDoc?.group_id === groupId ? chetGeneration(state.groupDoc) : "",
    );
    const senderSnapshotTitle = isChetMessage(ev, groupGeneration)
      ? CHET_NAME
      : typeof msgData?.sender_title === "string"
        ? String(msgData.sender_title || "").trim()
        : "";
    const remoteSourceName =
      typeof msgData?.source_user_name === "string"
        ? String(msgData.source_user_name || "").trim()
        : "";
    const remoteSourceId = String(msgData?.source_user_id || "").trim();
    const senderSnapshotRuntime =
      typeof msgData?.sender_runtime === "string"
        ? String(msgData.sender_runtime || "").trim()
        : "";
    const senderSnapshotAvatarPath =
      typeof msgData?.sender_avatar_path === "string"
        ? String(msgData.sender_avatar_path || "").trim()
        : "";
    const displayedMessageMode = projectMessageMode(msgData);
    const replyRequested = displayedMessageMode === "request_reply";
    // With 'Waiting for you' indicators off, a request to the user reads as an
    // ordinary message; a request between members keeps its mark.
    const replyMarked = replyRequested && !(userRequest && !showPendingPrompts);
    const isMail = displayedMessageMode === "mail";
    const srcGroupId =
      typeof msgData?.src_group_id === "string" ? String(msgData.src_group_id || "").trim() : "";
    const srcEventId =
      typeof msgData?.src_event_id === "string" ? String(msgData.src_event_id || "").trim() : "";
    const hasSource = !!(srcGroupId && srcEventId);
    const dstGroupId =
      typeof msgData?.dst_group_id === "string" ? String(msgData.dst_group_id || "").trim() : "";
    const dstTo = useMemo(() => {
      return projectCrossGroupRecipients(msgData);
    }, [msgData]);
    const hasDestination = !!dstGroupId;
    const rawAttachments: MessageAttachment[] = Array.isArray(msgData?.attachments)
      ? msgData.attachments
      : [];
    const sourcePlatform =
      typeof msgData?.source_platform === "string"
        ? String(msgData.source_platform || "").trim()
        : "";
    const isRemoteSource = isCrossInstanceInboundMessage(ev.by, msgData);
    const blobAttachments = rawAttachments
      .filter((a): a is MessageAttachment => a != null && typeof a === "object")
      .map((a) => ({
        kind: String(a.kind || "file"),
        path: String(a.path || ""),
        title: String(a.title || ""),
        bytes: Number(a.bytes || 0),
        mime_type: String(a.mime_type || ""),
        local_preview_url: "local_preview_url" in a ? String(a.local_preview_url || "") : "",
      }))
      .filter((a) => a.path.startsWith("state/blobs/") || a.local_preview_url.startsWith("blob:"));
    const displayMessageText = useMemo(() => {
      if (isRedundantWecomImagePlaceholder(messageText, blobAttachments, sourcePlatform)) {
        return "";
      }
      return messageText;
    }, [blobAttachments, messageText, sourcePlatform]);
    const delegationSourceOutbound = useMemo(
      () => isDelegationSourceOutbound({ rawText: displayMessageText, srcGroupId, dstGroupId }),
      [displayMessageText, dstGroupId, srcGroupId],
    );
    // Body text shown in the bubble: source-side outbound delegation is a
    // status, not conversation content. Target-side inbound delegation still
    // shows the natural contact body while hiding the protocol comment.
    const bubbleBodyText = useMemo(() => {
      if (replyView?.question) {
        if (replyView.state === "pending")
          return [replyView.question.context, replyView.question.prompt]
            .filter(Boolean)
            .join("\n\n");
        if (replyView.state === "answered" || replyView.state === "withdrawn")
          return recordedQuestionText(replyView.question, t("pendingReply.recommended"));
      }
      if (delegationSourceOutbound) return t("delegationOutboundStatus");
      const receipt = isUserMessage
        ? localizedAnswerReceipt(ev.data, displayMessageText, {
            selected: (optionId) => t("answerReceiptSelected", { id: optionId }),
            other: t("answerReceiptOther"),
          })
        : null;
      return receipt ?? getDelegationDisplayText(displayMessageText);
    }, [
      delegationSourceOutbound,
      displayMessageText,
      ev.data,
      isUserMessage,
      replyView?.state,
      replyView?.question,
      t,
    ]);
    const presentationRefs = useMemo(
      () => getPresentationMessageRefs(msgData?.refs),
      [msgData?.refs],
    );
    const taskRefs = useMemo(() => getTaskMessageRefs(msgData?.refs), [msgData?.refs]);
    const voiceDocumentRefs = useMemo(
      () => getVoiceDocumentMessageRefs(msgData?.refs),
      [msgData?.refs],
    );
    const shouldRenderMarkdown = useMemo(
      () => !isStreaming && mayContainMarkdown(bubbleBodyText),
      [bubbleBodyText, isStreaming],
    );
    const stableMessageAttachmentKey = useMemo(() => {
      const clientId =
        typeof msgData?.client_id === "string" ? String(msgData.client_id || "").trim() : "";
      if (clientId) return `client:${clientId}`;
      const eventId = typeof ev.id === "string" ? String(ev.id || "").trim() : "";
      return eventId || `row:${String(ev.ts || "")}:${String(ev.by || "")}`;
    }, [ev.id, ev.ts, ev.by, msgData]);
    const copyableMessageText = useMemo(
      () =>
        buildMessageCopyText({
          quoteText,
          messageText: displayMessageText,
          insight,
          insightLabel: t("senderPerspective"),
          presentationRefs,
          voiceDocumentRefs,
          taskRefs,
          attachments: blobAttachments.map((attachment) => ({
            title: attachment.title,
            path: attachment.path || attachment.local_preview_url,
          })),
        }),
      [
        blobAttachments,
        displayMessageText,
        insight,
        presentationRefs,
        quoteText,
        t,
        taskRefs,
        voiceDocumentRefs,
      ],
    );
    const messageTimestamp = formatMessageTimestamp(ev.ts);
    const fullMessageTimestamp = formatFullTime(ev.ts);

    // Use event's group_id for blob URLs (attachments are stored in the event's original group)
    const blobGroupId = String(ev.group_id || "").trim() || groupId;

    const readStatus = ev._read_status;
    const recipients = msgData?.to;

    const visibleReadStatusEntries = useMemo(() => {
      return buildVisibleReadStatusEntries(actors, readStatus);
    }, [actors, readStatus]);

    const isDirectUserMessage = useMemo(() => {
      if (isUserMessage) return false;
      if (!Array.isArray(recipients)) return false;
      const ids = recipients.map((id) => String(id || "").trim()).filter((id) => id);
      return ids.length === 1 && ids[0] === "user";
    }, [isUserMessage, recipients]);

    const hideDirectUserObligationSummary = useMemo(() => {
      if (isUserMessage) return false;
      const os = ev._obligation_status;
      if (os && typeof os === "object") {
        const ids = Object.keys(os);
        return ids.length === 1 && ids[0] === "user";
      }
      return isDirectUserMessage;
    }, [ev._obligation_status, isDirectUserMessage, isUserMessage]);

    const obligationSummary = useMemo(() => {
      return computeObligationSummary({
        hideDirectUserObligationSummary,
        obligationStatus: ev._obligation_status,
      });
    }, [ev._obligation_status, hideDirectUserObligationSummary]);

    const destinationLabel = msgData?.dst_instance_id
      ? connectGroupLabel(dstGroupId, msgData.dst_group_title, msgData.dst_instance_name)
      : groupLabelById[dstGroupId] || dstGroupId;
    const destinationNames = useMemo(
      () =>
        msgData?.dst_instance_id
          ? new Map(Object.entries(msgData.dst_actor_titles || {}))
          : displayNameMap,
      [msgData?.dst_instance_id, msgData?.dst_actor_titles, displayNameMap],
    );
    // A spectator of a read-only exhibit is not the Group's user.
    const userRecipientLabel = readOnly ? t("senderUser") : t("messageAuthorYou");
    const toLabel = useMemo(() => {
      return buildToLabel({
        hasDestination,
        dstGroupId,
        dstTo,
        groupLabelById,
        recipients,
        displayNameMap: destinationNames,
        userLabel: userRecipientLabel,
      });
    }, [
      destinationNames,
      dstGroupId,
      dstTo,
      groupLabelById,
      hasDestination,
      recipients,
      userRecipientLabel,
    ]);

    // Sender display name (use title if available)
    const senderActor = useMemo(() => {
      if (isUserMessage) return null;
      const senderId = String(ev.by || "").trim();
      if (!senderId) return null;
      return actorById.get(senderId) || null;
    }, [actorById, ev.by, isUserMessage]);

    const senderDisplayName = useMemo(() => {
      return getSenderDisplayName({
        senderId: String(ev.by || ""),
        senderActor,
        senderTitle: senderSnapshotTitle,
        remoteSourceName: isRemoteSource
          ? remoteSourceName || remoteSourceId || t("remoteGroupFallback")
          : remoteSourceName,
        groupLabelById,
        displayNameMap,
      });
    }, [
      displayNameMap,
      ev.by,
      groupLabelById,
      remoteSourceName,
      remoteSourceId,
      isRemoteSource,
      senderActor,
      senderSnapshotTitle,
      t,
    ]);
    const senderAvatarUrl = useMemo(() => {
      return (
        buildSenderAvatarUrl(blobGroupId, senderSnapshotAvatarPath) ||
        String(senderActor?.avatar_url || "").trim()
      );
    }, [blobGroupId, senderActor?.avatar_url, senderSnapshotAvatarPath]);
    const senderRuntime = senderSnapshotRuntime || String(senderActor?.runtime || "").trim();
    const sourceLabel = useMemo(() => {
      if (msgData?.src_instance_id)
        return connectGroupLabel(srcGroupId, msgData.src_group_title, msgData.src_instance_name);
      if (!hasSource || isRemoteSource) return "";
      return String(groupLabelById?.[srcGroupId] || "").trim() || remoteSourceName || srcGroupId;
    }, [
      remoteSourceName,
      groupLabelById,
      hasSource,
      isRemoteSource,
      srcGroupId,
      msgData?.src_instance_id,
      msgData?.src_group_title,
      msgData?.src_instance_name,
    ]);
    const sourceTitle = useMemo(() => {
      if (msgData?.src_instance_id) return `${msgData.src_instance_id} / ${srcGroupId}`;
      if (!hasSource || isRemoteSource) return "";
      return t("relayedSourceDetails", {
        label: sourceLabel,
        groupId: srcGroupId,
        eventId: srcEventId,
      });
    }, [
      hasSource,
      isRemoteSource,
      sourceLabel,
      srcEventId,
      srcGroupId,
      t,
      msgData?.src_instance_id,
    ]);
    const remoteBadgeLabel = isRemoteSource ? t("remoteBadge", { defaultValue: "Remote" }) : "";

    const readPreviewEntries = visibleReadStatusEntries.slice(0, 3);
    const readPreviewOverflow = Math.max(
      0,
      visibleReadStatusEntries.length - readPreviewEntries.length,
    );

    // The copy actions sit in a menu that closes on use, so the notice confirms it.
    const handleCopyMessageText = useCallback(async () => {
      await copyWithFeedback(copyableMessageText, {
        successMessage: t("textCopied"),
        errorMessage: t("common:copyFailed", { defaultValue: "Copy failed" }),
      });
    }, [copyWithFeedback, copyableMessageText, t]);

    // A quick reply is a Reply sent at once. It is offered on a message from a current agent of
    // this Group where Reply would address that agent alone, in this Group; Chet, IM users and
    // other Groups or instances write under IDs that name no member. A message relayed from
    // another Group, even by a member, one still being written and a question with its own
    // answer card keep to the composer.
    const quickReplyTarget = useMemo<QuickReplyTarget | null>(() => {
      if (!senderActor || isStreaming || srcGroupId || msgData?.question) return null;
      const reply = buildReplyComposerState(ev, groupId, actors, null, groupGeneration);
      const target = reply?.replyTarget;
      const author = String(ev.by || "");
      if (!reply || !target || reply.toText !== author || target.remoteDstGroupId) return null;
      return {
        groupId: reply.destGroupId,
        replyEventId: target.eventId,
        to: [author],
        quoteText: target.text,
      };
    }, [actors, ev, groupGeneration, groupId, isStreaming, msgData, senderActor, srcGroupId]);
    const showNotice = useUIStore((state) => state.showNotice);
    const quickReplies = useMemo(() => {
      if (!quickReplyTarget) return undefined;
      const send = async (kind: QuickReplyKind) => {
        const label = t(`quickReply.${kind}`);
        const result = await sendQuickReply(quickReplyTarget, kind, t(`quickReply.${kind}Text`));
        if (result.status === "sent") {
          showNotice({ message: t("quickReply.sent", { name: senderDisplayName, label }) });
        } else if (result.status === "failed") {
          // A retry from the notice reuses the client ID, even after this row has scrolled
          // away and unmounted: the quick reply module keeps it until the reply is sent.
          showNotice({
            message: result.unknown
              ? t("quickReply.unknown")
              : t("quickReply.failed", { error: result.message }),
            actionLabel: t("quickReply.retry"),
            onAction: () => void send(kind),
          });
        }
      };
      return QUICK_REPLY_KINDS.map((kind) => ({
        key: `quick-${kind}`,
        label: t(`quickReply.${kind}`),
        emoji: QUICK_REPLY_EMOJI[kind],
        run: () => void send(kind),
      }));
    }, [quickReplyTarget, senderDisplayName, showNotice, t]);

    const routingMetadata = (
      <MessageRoutingMetadata
        toLabel={toLabel}
        isMail={isMail}
        hasSource={hasSource}
        sourceLabel={sourceLabel}
        sourceTitle={sourceTitle}
        isRemoteSource={isRemoteSource}
        srcGroupId={srcGroupId}
        srcEventId={srcEventId}
        hasDestination={hasDestination}
        destinationLabel={destinationLabel}
        dstTo={dstTo.map((id) => destinationNames.get(id) || id)}
        messageText={displayMessageText}
        onOpenSource={onOpenSource}
      />
    );

    const agentStateTooltip =
      isAgentStateOpen && canShowAgentState && !collapseHeader ? (
        <FloatingPortal>
          <AgentStateTooltip
            isOpen={true}
            canShow={true}
            isPositioned={isAgentStatePositioned}
            setFloating={setAgentStateFloating}
            floatingStyles={floatingStyles}
            senderDisplayName={senderDisplayName}
            updatedAt={agentState?.updated_at ? String(agentState.updated_at) : undefined}
            agentStateDisplay={agentStateDisplay}
            stateTask={stateTask}
            blockerCount={blockerCount}
            stateNext={stateNext}
            stateChanged={stateChanged}
          />
        </FloatingPortal>
      ) : null;

    const bubbleContent = (
      <>
        <MessageBubbleBody
          event={ev}
          messageContent={
            <MessageContent
              fallbackText={bubbleBodyText}
              shouldRenderMarkdown={shouldRenderMarkdown}
              isDark={isDark}
              knownTokens={mentionTokens}
              animateMentions={!isStreaming}
            />
          }
          isUserMessage={isUserMessage}
          isDark={isDark}
          quoteText={quoteText}
          replyToEventId={replyToEventId}
          presentationRefs={presentationRefs}
          voiceDocumentRefs={voiceDocumentRefs}
          taskRefs={taskRefs}
          taskById={taskById}
          insight={insight}
          blobAttachments={blobAttachments}
          blobGroupId={blobGroupId}
          stableMessageAttachmentKey={stableMessageAttachmentKey}
          onOpenPresentationRef={onOpenPresentationRef}
          onOpenTaskRef={onOpenTaskRef}
          onOpenReplyTarget={onOpenReplyTarget}
        />
        <PendingReplySlot
          event={replyEvent}
          groupId={groupId}
          readOnly={readOnly}
          checking={liveEntry?.checking}
          onUseComposer={onReply}
        />
      </>
    );

    const messageFooter = (
      <MessageFooter
        readOnly={readOnly}
        obligationSummary={obligationSummary}
        visibleReadStatusEntries={visibleReadStatusEntries}
        webModelDeliveryStatus={webModelDeliveryStatus}
        readPreviewEntries={readPreviewEntries}
        readPreviewOverflow={readPreviewOverflow}
        displayNameMap={displayNameMap}
        isDark={isDark}
        onShowRecipients={onShowRecipients}
        event={ev}
      />
    );

    const showAgentState = canShowAgentState && !collapseHeader;
    // A read-only exhibit is watched by spectators, not by the Group's user, so the
    // user's messages are neither "you" nor set on the reader's side; spectators see
    // the role in their own language.
    const fromReader = isUserMessage && !readOnly;
    const lcdAuthorName = fromReader
      ? t("messageAuthorYou")
      : isUserMessage
        ? t("senderUser")
        : senderDisplayName;
    const messageActionProps = {
      readOnly,
      copyableMessageText,
      onCopyMessageText: () => void handleCopyMessageText(),
      onShowInConversation: showInConversation,
      onCopyLink,
      onRelay,
      onReply,
      canReply,
      eventId: typeof ev.id === "string" ? String(ev.id) : undefined,
      event: ev,
      quickReplies,
      pinned,
      onTogglePin: pinnableId && onTogglePin ? () => onTogglePin(pinnableId, !pinned) : undefined,
    };
    return (
      <div
        className={classNames(
          "message-row lcd-row group",
          isOptimistic ? "opacity-95" : "",
          isHighlighted ? "lcd-row-highlighted" : "",
        )}
        data-lcd-row
        data-lcd-user={fromReader || undefined}
        data-lcd-continued={collapseHeader || undefined}
        data-lcd-same-sender={sameSender || undefined}
        data-lcd-show-in-chat={showInConversation ? true : undefined}
        data-lcd-accent={isUserMessage ? undefined : getActorAccentIndex(String(ev.by || ""))}
      >
        {/* One head line, so names never truncate and the text keeps the full width. */}
        <div className="lcd-row-head">
          {collapseHeader ? (
            <span className="sr-only">{lcdAuthorName}</span>
          ) : (
            <span
              className="lcd-row-who"
              ref={showAgentState ? setAgentStateReference : undefined}
              onMouseEnter={showAgentState ? () => setIsAgentStateOpen(true) : undefined}
              onMouseLeave={showAgentState ? () => setIsAgentStateOpen(false) : undefined}
              onFocus={showAgentState ? () => setIsAgentStateOpen(true) : undefined}
              onBlur={showAgentState ? () => setIsAgentStateOpen(false) : undefined}
              tabIndex={showAgentState ? 0 : undefined}
              aria-label={
                showAgentState
                  ? t("agentStateTooltipLabel", { defaultValue: "View agent state" })
                  : undefined
              }
            >
              {isUserMessage ? null : (
                <ActorAvatar
                  avatarUrl={senderAvatarUrl || undefined}
                  runtime={senderRuntime || undefined}
                  title={senderDisplayName}
                  isDark={isDark}
                  sizeClassName="h-5 w-5"
                  textClassName="text-[0.6875rem]"
                  className="lcd-tile"
                />
              )}
              <span className="lcd-row-name">{lcdAuthorName}</span>
            </span>
          )}
          {remoteBadgeLabel ? <span className="lcd-tag">{remoteBadgeLabel}</span> : null}
          <span className="lcd-row-route">
            {routingMetadata}
            {/* The reply card below already shows the request and its state. */}
            {replyMarked && !replyView ? <span className="lcd-tag">{t("needReply")}</span> : null}
          </span>
          <time
            className="lcd-row-time"
            dateTime={String(ev.ts || "")}
            title={fullMessageTimestamp}
          >
            {messageTimestamp}
          </time>
          {pinned ? (
            <span className="lcd-row-pin" title={t("pins.pinnedMark")}>
              <PinIcon size={12} aria-hidden="true" />
              <span className="sr-only">{t("pins.pinnedMark")}</span>
            </span>
          ) : null}
          {/* Where messages have no hover bar to carry it, or no room on it (lcd.css), a
              filtered view shows its way into the whole conversation here, after the time. */}
          {showInConversation ? (
            <button
              type="button"
              className="lcd-row-jump"
              onClick={() => showInConversation(String(ev.id))}
            >
              {t("showInConversation")}
            </button>
          ) : null}
          <MessageActions part="bar" {...messageActionProps} />
          <MessageActions part="menu" {...messageActionProps} />
        </div>
        {agentStateTooltip}
        <div className="lcd-row-body">
          <div className={classNames("lcd-row-content", isStreaming ? "lcd-row-streaming" : "")}>
            {bubbleContent}
          </div>
          {messageFooter}
        </div>
      </div>
    );
  },
  (prevProps, nextProps) => {
    return (
      prevProps.event === nextProps.event &&
      prevProps.actors === nextProps.actors &&
      prevProps.displayNameMap === nextProps.displayNameMap &&
      prevProps.agentState === nextProps.agentState &&
      prevProps.taskById === nextProps.taskById &&
      prevProps.isDark === nextProps.isDark &&
      prevProps.readOnly === nextProps.readOnly &&
      prevProps.groupId === nextProps.groupId &&
      prevProps.groupLabelById === nextProps.groupLabelById &&
      prevProps.webModelDeliveryStatus === nextProps.webModelDeliveryStatus &&
      prevProps.isHighlighted === nextProps.isHighlighted &&
      prevProps.collapseHeader === nextProps.collapseHeader &&
      prevProps.sameSender === nextProps.sameSender &&
      prevProps.onViewContext === nextProps.onViewContext &&
      prevProps.onTogglePin === nextProps.onTogglePin &&
      prevProps.onRelay === nextProps.onRelay &&
      prevProps.onOpenSource === nextProps.onOpenSource &&
      prevProps.onOpenPresentationRef === nextProps.onOpenPresentationRef &&
      prevProps.onOpenTaskRef === nextProps.onOpenTaskRef &&
      prevProps.onOpenReplyTarget === nextProps.onOpenReplyTarget
    );
  },
);
