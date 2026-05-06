import { memo, useCallback, useEffect, useMemo, useState } from "react";
import type { CSSProperties } from "react";
import { FloatingPortal, autoUpdate, flip, offset, shift, useFloating } from "@floating-ui/react";
import { useTranslation } from "react-i18next";
import { useCopyFeedback } from "../hooks/useCopyFeedback";
import { LedgerEvent, Actor, AgentState, Task, getActorAccentColor, ChatMessageData, MessageAttachment, PresentationMessageRef, TaskMessageRef } from "../types";
import { formatFullTime, formatMessageTimestamp, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { getReplyEventId } from "../utils/chatReply";
import { getPresentationMessageRefs, getPresentationRefChipLabel } from "../utils/presentationRefs";
import { getTaskMessageRefs, getTaskRefChipLabel, getTaskRefStateKey, type TaskRefStateKey } from "../utils/taskRefs";
import { isRedundantWecomImagePlaceholder } from "../utils/messageAttachments";
import { MessageAttachments } from "./messageBubble/MessageAttachments";
import { MessageFooter, MessageMetadataHeader } from "./messageBubble/MessageBubbleChrome";
import { withAuthToken } from "../services/api/base";
import {
    buildToLabel,
    buildVisibleReadStatusEntries,
    computeAckSummary,
    computeObligationSummary,
    getSenderDisplayName,
} from "./messageBubble/model";
import { ActorAvatar } from "./ActorAvatar";
import {
    formatEventLine,
    getMessageBubbleMotionClass,
    mayContainMarkdown,
} from "./messageBubble/helpers";
import { LazyMarkdownRenderer } from "./LazyMarkdownRenderer";

const ANIMATED_MESSAGE_BUBBLE_KEYS = new Set<string>();
const NEW_MESSAGE_ANIMATION_WINDOW_MS = 12000;

const TASK_REF_STATE_TONE_CLASS: Record<TaskRefStateKey, string> = {
    planned: "border-slate-300/70 bg-slate-100/90 text-slate-700 dark:border-white/10 dark:bg-white/[0.08] dark:text-slate-300",
    active: "border-emerald-300/70 bg-emerald-100/90 text-emerald-700 dark:border-emerald-500/30 dark:bg-emerald-500/12 dark:text-emerald-300",
    handoff: "border-sky-300/70 bg-sky-100/90 text-sky-700 dark:border-sky-500/30 dark:bg-sky-500/12 dark:text-sky-300",
    waiting_user: "border-amber-300/70 bg-amber-100/90 text-amber-700 dark:border-amber-500/30 dark:bg-amber-500/12 dark:text-amber-300",
    blocked: "border-rose-300/70 bg-rose-100/90 text-rose-700 dark:border-rose-500/30 dark:bg-rose-500/12 dark:text-rose-300",
    done: "border-emerald-300/60 bg-emerald-50/95 text-emerald-700 dark:border-emerald-500/25 dark:bg-emerald-500/10 dark:text-emerald-300",
    archived: "border-slate-300/70 bg-slate-100/90 text-slate-600 dark:border-white/10 dark:bg-white/[0.07] dark:text-slate-400",
    linked: "border-slate-300/70 bg-slate-100/90 text-slate-700 dark:border-white/10 dark:bg-white/[0.08] dark:text-slate-300",
};

const TASK_REF_STATE_DOT_CLASS: Record<TaskRefStateKey, string> = {
    planned: "bg-slate-400/90 dark:bg-slate-400",
    active: "bg-emerald-500 dark:bg-emerald-400",
    handoff: "bg-sky-500 dark:bg-sky-400",
    waiting_user: "bg-amber-500 dark:bg-amber-400",
    blocked: "bg-rose-500 dark:bg-rose-400",
    done: "bg-emerald-500 dark:bg-emerald-400",
    archived: "bg-slate-400/90 dark:bg-slate-500",
    linked: "bg-slate-400/90 dark:bg-slate-400",
};

function buildSenderAvatarUrl(groupId: string, senderAvatarPath?: string): string {
    const gid = String(groupId || "").trim();
    const relPath = String(senderAvatarPath || "").trim();
    if (!gid || !relPath.startsWith("state/blobs/")) return "";
    const blobName = relPath.split("/").pop() || "";
    if (!blobName) return "";
    return withAuthToken(`/api/v1/groups/${encodeURIComponent(gid)}/blobs/${encodeURIComponent(blobName)}`);
}

function shouldAnimateIncomingBubble(messageKey: string, eventTs?: string): boolean {
    const stableKey = String(messageKey || "").trim();
    if (!stableKey || ANIMATED_MESSAGE_BUBBLE_KEYS.has(stableKey)) return false;

    const parsedTs = Date.parse(String(eventTs || "").trim());
    if (!Number.isFinite(parsedTs)) {
        ANIMATED_MESSAGE_BUBBLE_KEYS.add(stableKey);
        return false;
    }

    if (Math.abs(Date.now() - parsedTs) > NEW_MESSAGE_ANIMATION_WINDOW_MS) {
        ANIMATED_MESSAGE_BUBBLE_KEYS.add(stableKey);
        return false;
    }

    ANIMATED_MESSAGE_BUBBLE_KEYS.add(stableKey);
    return true;
}

function PlainMessageText({
    text,
    className,
}: {
    text: string;
    className?: string;
}) {
    return (
        <div
            className={classNames(
                "break-words whitespace-pre-wrap text-[var(--color-text-primary)] [overflow-wrap:anywhere]",
                className
            )}
        >
            {text}
        </div>
    );
}

function buildMessageCopyText({
    quoteText,
    messageText,
    presentationRefs,
    taskRefs,
    attachments,
}: {
    quoteText?: string;
    messageText: string;
    presentationRefs: PresentationMessageRef[];
    taskRefs: TaskMessageRef[];
    attachments: { title: string; path: string }[];
}): string {
    const sections: string[] = [];
    const trimmedQuote = String(quoteText || "").trim();
    const trimmedMessage = String(messageText || "").trim();

    if (trimmedQuote) {
        sections.push(`> ${trimmedQuote}`);
    }
    if (trimmedMessage) {
        sections.push(trimmedMessage);
    }
    if (presentationRefs.length > 0) {
        sections.push([
            "Presentation refs:",
            ...presentationRefs.map((ref) => `- ${getPresentationRefChipLabel(ref)}`),
        ].join("\n"));
    }
    if (taskRefs.length > 0) {
        sections.push([
            "Tasks:",
            ...taskRefs.map((ref) => `- ${getTaskRefChipLabel(ref)}`),
        ].join("\n"));
    }
    if (attachments.length > 0) {
        sections.push([
            "Attachments:",
            ...attachments.map((attachment) => {
                const rawTitle = String(attachment.title || "").trim();
                if (rawTitle) return `- ${rawTitle}`;
                const parts = String(attachment.path || "").split("/");
                return `- ${parts[parts.length - 1] || "file"}`;
            }),
        ].join("\n"));
    }
    return sections.join("\n\n").trim();
}


function MessageBubbleBody({
    event,
    isUserMessage,
    isDark,
    groupLabelById,
    toLabel,
    hasSource,
    srcGroupId,
    srcEventId,
    hasDestination,
    dstGroupId,
    dstTo,
    relayChipClass,
    quoteText,
    replyToEventId,
    presentationRefs,
    taskRefs,
    taskById,
    messageText,
    shouldRenderMarkdown,
    blobAttachments,
    blobGroupId,
    stableMessageAttachmentKey,
    onOpenSource,
    onOpenPresentationRef,
    onOpenTaskRef,
    onOpenReplyTarget,
}: {
    event: LedgerEvent;
    isUserMessage: boolean;
    isDark: boolean;
    groupLabelById: Record<string, string>;
    toLabel: string;
    hasSource: boolean;
    srcGroupId: string;
    srcEventId: string;
    hasDestination: boolean;
    dstGroupId: string;
    dstTo: string[];
    relayChipClass: string;
    quoteText?: string;
    replyToEventId?: string;
    presentationRefs: PresentationMessageRef[];
    taskRefs: TaskMessageRef[];
    taskById: Map<string, Task>;
    messageText: string;
    shouldRenderMarkdown: boolean;
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
    onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
    onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
    onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
    onOpenReplyTarget?: (replyToEventId: string) => void;
}) {
    const { t } = useTranslation("chat");
    const canJumpToReplyTarget = !!(replyToEventId && onOpenReplyTarget);
    const quoteClassName = classNames(
        "rounded-2xl border px-3 py-2 text-[12px] leading-5",
        "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
    );
    const metaChipClass = classNames(
        "inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium",
        "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]"
    );
    const normalizedToLabel = String(toLabel || "").trim();
    const supportingSectionClass = classNames(
        "mt-3 border-t pt-3",
        "border-[var(--glass-border-subtle)]"
    );
    const taskStateLabels: Record<TaskRefStateKey, string> = {
        planned: t("taskRefStatePlanned", { defaultValue: "Planned" }),
        active: t("taskRefStateActive", { defaultValue: "Active" }),
        handoff: t("taskRefStateHandoff", { defaultValue: "Handoff" }),
        waiting_user: t("taskRefStateWaitingUser", { defaultValue: "Waiting user" }),
        blocked: t("taskRefStateBlocked", { defaultValue: "Blocked" }),
        done: t("taskRefStateDone", { defaultValue: "Done" }),
        archived: t("taskRefStateArchived", { defaultValue: "Archived" }),
        linked: t("taskRefStateLinked", { defaultValue: "Linked" }),
    };

    return (
        <>
            {(normalizedToLabel || hasSource || hasDestination) ? (
                <div className="mb-3 flex flex-wrap items-center gap-1.5">
                    {normalizedToLabel ? (
                        <span className={metaChipClass} title={normalizedToLabel}>
                            <span className="opacity-55">{t("to")}</span>
                            <span className="truncate">{normalizedToLabel}</span>
                        </span>
                    ) : null}
                    {hasSource ? (
                        <button
                            type="button"
                            className={classNames(
                                metaChipClass,
                                relayChipClass,
                                onOpenSource ? "cursor-pointer transition-colors hover:opacity-100" : "cursor-default"
                            )}
                            onClick={() => onOpenSource?.(srcGroupId, srcEventId)}
                            disabled={!onOpenSource}
                            title={t("openOriginalMessage")}
                        >
                            <span className="opacity-65">↗</span>
                            <span className="truncate">
                                {t("relayedFrom", { groupId: srcGroupId, eventId: srcEventId.slice(0, 8) })}
                            </span>
                        </button>
                    ) : null}
                    {hasDestination ? (() => {
                        const dstLabel = String(groupLabelById?.[dstGroupId] || "").trim() || dstGroupId;
                        const dstToLabel = dstTo.length > 0 ? dstTo.join(", ") : "@all";
                        return (
                            <div
                                className={classNames(metaChipClass, relayChipClass)}
                                title={t("sentTo", { label: dstGroupId, to: dstToLabel })}
                            >
                                <span className="opacity-65">↗</span>
                                <span className="truncate">
                                    {t("sentTo", { label: dstLabel, to: dstToLabel })}
                                </span>
                            </div>
                        );
                    })() : null}
                </div>
            ) : null}

            {quoteText ? (
                canJumpToReplyTarget ? (
                    <button
                        type="button"
                        className={classNames(
                            quoteClassName,
                            "mb-3 block w-full cursor-pointer appearance-none bg-transparent text-left text-inherit transition-opacity hover:opacity-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(35,36,37)]/20 dark:focus-visible:ring-white/25"
                        )}
                        onClick={(mouseEvent) => {
                            mouseEvent.stopPropagation();
                            onOpenReplyTarget?.(String(replyToEventId || ""));
                        }}
                        title={t("jumpToRepliedMessage")}
                        aria-label={t("jumpToRepliedMessage")}
                    >
                        <span className="mb-1 block text-[10px] font-semibold uppercase tracking-[0.14em] opacity-55">
                            {t("reply")}
                        </span>
                        <span className="block">"{quoteText}"</span>
                    </button>
                ) : (
                    <div className={classNames(quoteClassName, "mb-3")}>
                        <span className="mb-1 block text-[10px] font-semibold uppercase tracking-[0.14em] opacity-55">
                            {t("reply")}
                        </span>
                        <span className="block">"{quoteText}"</span>
                    </div>
                )
            ) : null}

            {presentationRefs.length > 0 ? (
                <div className={supportingSectionClass}>
                    <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] opacity-50">
                        {t("presentation")}
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                    {presentationRefs.map((ref, index) => (
                        <button
                            key={`${String(event.id || "message")}:presentation-ref:${index}:${String(ref.slot_id || "")}`}
                            type="button"
                            onClick={() => onOpenPresentationRef?.(ref, event)}
                            className={classNames(
                                "inline-flex max-w-full items-center rounded-full border px-2.5 py-1 text-[11px] font-medium transition-colors",
                                "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
                            )}
                            title={getPresentationRefChipLabel(ref)}
                        >
                            <span className="truncate">{getPresentationRefChipLabel(ref)}</span>
                        </button>
                    ))}
                    </div>
                </div>
            ) : null}

            {taskRefs.length > 0 ? (
                <div className={supportingSectionClass}>
                    <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] opacity-50">
                        {t("task", { defaultValue: "Task" })}
                    </div>
                    <div className="flex flex-wrap gap-1.5">
                        {taskRefs.map((ref, index) => {
                            const taskId = String(ref.task_id || "").trim();
                            const liveTask = taskId ? (taskById.get(taskId) || null) : null;
                            const stateKey = getTaskRefStateKey(ref, liveTask);
                            const stateLabel = taskStateLabels[stateKey];
                            const chipLabel = getTaskRefChipLabel(ref, liveTask);
                            return (
                                <button
                                    key={`${String(event.id || "message")}:task-ref:${index}:${taskId}`}
                                    type="button"
                                    onClick={() => onOpenTaskRef?.(ref, event)}
                                    className={classNames(
                                        "inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium transition-colors",
                                        "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]"
                                    )}
                                    title={`${chipLabel} · ${stateLabel}`}
                                >
                                    <span className={classNames("h-1.5 w-1.5 rounded-full", TASK_REF_STATE_DOT_CLASS[stateKey])} aria-hidden="true" />
                                    <span className="truncate">{chipLabel}</span>
                                    <span
                                        className={classNames(
                                            "shrink-0 rounded-full border px-1.5 py-0.5 text-[10px] font-semibold leading-none",
                                            TASK_REF_STATE_TONE_CLASS[stateKey]
                                        )}
                                    >
                                        {stateLabel}
                                    </span>
                                </button>
                            );
                        })}
                    </div>
                </div>
            ) : null}

            <MessageContent
                fallbackText={messageText}
                shouldRenderMarkdown={shouldRenderMarkdown}
                isDark={isDark}
            />

            <MessageAttachments
                attachments={blobAttachments}
                blobGroupId={blobGroupId}
                isUserMessage={isUserMessage}
                isDark={isDark}
                attachmentKeyPrefix={stableMessageAttachmentKey}
                downloadTitle={(name) => t("download", { name })}
                sectionClassName={supportingSectionClass}
            />
        </>
    );
}

function MessageContent({
    fallbackText,
    shouldRenderMarkdown,
    isDark,
}: {
    fallbackText: string;
    shouldRenderMarkdown: boolean;
    isDark: boolean;
}) {
    if (shouldRenderMarkdown) {
        return (
            <LazyMarkdownRenderer
                content={fallbackText}
                isDark={isDark}
                invertText={false}
                className="max-w-full break-words text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
                fallback={
                    <PlainMessageText
                        text={fallbackText}
                        className="max-w-full"
                    />
                }
            />
        );
    }

    return (
        <PlainMessageText
            text={fallbackText}
            className="max-w-full"
        />
    );
}

function AgentStateTooltip({
    isOpen,
    canShow,
    isPositioned,
    setFloating,
    floatingStyles,
    senderDisplayName,
    updatedAt,
    agentStateDisplay,
    stateTask,
    blockerCount,
    stateNext,
    stateChanged,
}: {
    isOpen: boolean;
    canShow: boolean;
    isPositioned: boolean;
    setFloating: (node: HTMLElement | null) => void;
    floatingStyles: CSSProperties;
    senderDisplayName: string;
    updatedAt?: string;
    agentStateDisplay: string;
    stateTask: string;
    blockerCount: number;
    stateNext: string;
    stateChanged: string;
}) {
    const { t } = useTranslation("chat");

    if (!isOpen || !canShow) return null;

    return (
        <div
            ref={setFloating}
            style={floatingStyles}
            className={classNames(
                "pointer-events-none z-[80] w-[min(360px,calc(100vw-32px))] rounded-2xl px-3 py-2 shadow-2xl transition-opacity duration-150",
                "glass-modal text-[var(--color-text-primary)]",
                isPositioned ? "opacity-100" : "opacity-0"
            )}
            role="status"
        >
            <div className="flex items-center gap-2">
                <div className="text-xs font-semibold text-[var(--color-text-primary)]">
                    {senderDisplayName}
                </div>
                {updatedAt ? (
                    <div
                        className={classNames(
                            "ml-auto text-xs tabular-nums",
                            "text-[var(--color-text-tertiary)]"
                        )}
                        title={formatFullTime(updatedAt)}
                    >
                        {t("updated", { time: formatTime(updatedAt) })}
                    </div>
                ) : null}
            </div>
            <div className="mt-1 text-xs whitespace-pre-wrap text-[var(--color-text-secondary)]">
                {agentStateDisplay}
            </div>
            {(stateTask || blockerCount > 0 || stateNext || stateChanged) ? (
                <div className="mt-2 space-y-1">
                    <div className="flex flex-wrap items-center gap-1.5">
                        {stateTask ? (
                            <span className="text-[11px] px-2 py-0.5 rounded bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]">
                                {t("taskShort", { id: stateTask })}
                            </span>
                        ) : null}
                        {blockerCount > 0 ? (
                            <span className="text-[11px] px-2 py-0.5 rounded bg-rose-500/15 text-rose-600 dark:text-rose-300">
                                {t("blockersShort", { count: blockerCount })}
                            </span>
                        ) : null}
                    </div>
                    {stateNext ? (
                        <div className="text-[11px] text-[var(--color-text-tertiary)]">
                            {t("nextShort", { value: stateNext })}
                        </div>
                    ) : null}
                    {stateChanged ? (
                        <div className={classNames("text-[11px]", "text-[var(--color-text-tertiary)]")}>
                            {t("changedShort", { value: stateChanged })}
                        </div>
                    ) : null}
                </div>
            ) : null}
        </div>
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
    isHighlighted?: boolean;
    collapseHeader?: boolean;
    onReply: () => void;
    onShowRecipients: () => void;
    onCopyLink?: (eventId: string) => void;
    onCopyContent?: (ev: LedgerEvent) => void;
    onRelay?: (ev: LedgerEvent) => void;
    onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
    onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
    onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
    onOpenReplyTarget?: (replyToEventId: string) => void;
}

export const MessageBubble = memo(function MessageBubble({
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
    isHighlighted,
    collapseHeader,
    onReply,
    onShowRecipients,
    onCopyLink,
    onCopyContent: _onCopyContent,
    onRelay,
    onOpenSource,
    onOpenPresentationRef,
    onOpenTaskRef,
    onOpenReplyTarget,
}: MessageBubbleProps) {
    const isUserMessage = ev.by === "user";
    const isOptimistic = !!(ev.data as Record<string, unknown> | undefined)?._optimistic;
    const senderAccent = !isUserMessage ? getActorAccentColor(String(ev.by || ""), isDark) : null;
    const isStreaming = !!ev._streaming;
    const canReply = !!getReplyEventId(ev);
    const messageText = useMemo(() => formatEventLine(ev), [ev]);

    const [isAgentStateOpen, setIsAgentStateOpen] = useState(false);
    const [copiedMessageText, setCopiedMessageText] = useState(false);
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
    const setAgentStateReference = useCallback((node: HTMLElement | null) => {
        refs.setReference(node);
    }, [refs]);
    const setAgentStateFloating = useCallback((node: HTMLElement | null) => {
        refs.setFloating(node);
    }, [refs]);

    const canShowAgentState = useMemo(() => {
        if (isUserMessage) return false;
        const id = String(ev.by || "");
        if (!id) return false;
        return true;
    }, [ev.by, isUserMessage]);

    const { t } = useTranslation('chat');
    const copyWithFeedback = useCopyFeedback();

    const agentStateText = String(agentState?.hot?.focus || "").trim();
    const agentStateDisplay = agentStateText || t('noAgentStateYet');
    const stateTask = String(agentState?.hot?.active_task_id || "").trim();
    const stateNext = String(agentState?.hot?.next_action || "").trim();
    const stateChanged = String(agentState?.warm?.what_changed || "").trim();
    const blockerCount = Array.isArray(agentState?.hot?.blockers) ? agentState.hot.blockers.length : 0;


    // Treat data as ChatMessageData.
    const msgData = ev.data as ChatMessageData | undefined;
    const quoteText = msgData?.quote_text;
    const replyToEventId = typeof msgData?.reply_to === "string" ? String(msgData.reply_to || "").trim() : "";
    const senderSnapshotTitle = typeof msgData?.sender_title === "string" ? String(msgData.sender_title || "").trim() : "";
    const senderSnapshotRuntime = typeof msgData?.sender_runtime === "string" ? String(msgData.sender_runtime || "").trim() : "";
    const senderSnapshotAvatarPath = typeof msgData?.sender_avatar_path === "string" ? String(msgData.sender_avatar_path || "").trim() : "";
    const isAttention = String(msgData?.priority || "normal") === "attention";
    const replyRequired = !!msgData?.reply_required;
    const srcGroupId = typeof msgData?.src_group_id === "string" ? String(msgData.src_group_id || "").trim() : "";
    const srcEventId = typeof msgData?.src_event_id === "string" ? String(msgData.src_event_id || "").trim() : "";
    const hasSource = !!(srcGroupId && srcEventId);
    const dstGroupId = typeof msgData?.dst_group_id === "string" ? String(msgData.dst_group_id || "").trim() : "";
    const dstTo = useMemo(() => {
        const raw = msgData?.dst_to;
        if (!Array.isArray(raw)) return [];
        return raw.map((t) => String(t || "").trim()).filter((t) => t);
    }, [msgData?.dst_to]);
    const hasDestination = !!dstGroupId;
    const rawAttachments: MessageAttachment[] = Array.isArray(msgData?.attachments) ? msgData.attachments : [];
    const sourcePlatform = typeof msgData?.source_platform === "string" ? String(msgData.source_platform || "").trim() : "";
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
    const presentationRefs = useMemo(() => getPresentationMessageRefs(msgData?.refs), [msgData?.refs]);
    const taskRefs = useMemo(() => getTaskMessageRefs(msgData?.refs), [msgData?.refs]);
    const shouldRenderMarkdown = useMemo(() => !isStreaming && mayContainMarkdown(displayMessageText), [displayMessageText, isStreaming]);
    const streamPhase = String((msgData as { stream_phase?: unknown } | undefined)?.stream_phase || "").trim().toLowerCase();
    const stableMessageAttachmentKey = useMemo(() => {
        const clientId = typeof msgData?.client_id === "string" ? String(msgData.client_id || "").trim() : "";
        if (clientId) return `client:${clientId}`;
        const eventId = typeof ev.id === "string" ? String(ev.id || "").trim() : "";
        return eventId || `row:${String(ev.ts || "")}:${String(ev.by || "")}`;
    }, [ev.id, ev.ts, ev.by, msgData]);
    const shouldAnimateBubbleOnEnter = useMemo(() => {
        return shouldAnimateIncomingBubble(stableMessageAttachmentKey, String(ev.ts || ""));
    }, [ev.ts, stableMessageAttachmentKey]);
    const bubbleMotionClass = useMemo(() => getMessageBubbleMotionClass({
        isStreaming,
        isOptimistic,
        isNewlyArrived: shouldAnimateBubbleOnEnter,
        isUserMessage,
        streamPhase,
    }), [isOptimistic, isStreaming, isUserMessage, shouldAnimateBubbleOnEnter, streamPhase]);
    const copyableMessageText = useMemo(
        () =>
            buildMessageCopyText({
                quoteText,
                messageText: displayMessageText,
                presentationRefs,
                taskRefs,
                attachments: blobAttachments.map((attachment) => ({
                    title: attachment.title,
                    path: attachment.path || attachment.local_preview_url,
                })),
            }),
        [blobAttachments, displayMessageText, presentationRefs, quoteText, taskRefs]
    );
    const messageTimestamp = formatMessageTimestamp(ev.ts);
    const fullMessageTimestamp = formatFullTime(ev.ts);

    // Use event's group_id for blob URLs (attachments are stored in the event's original group)
    const blobGroupId = String(ev.group_id || "").trim() || groupId;

    const readStatus = ev._read_status;
    const ackStatus = ev._ack_status;
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
        if (ackStatus && typeof ackStatus === "object") {
            const ids = Object.keys(ackStatus);
            return ids.length === 1 && ids[0] === "user";
        }
        return isDirectUserMessage;
    }, [ackStatus, ev._obligation_status, isDirectUserMessage, isUserMessage]);

    const ackSummary = useMemo(() => {
        return computeAckSummary({
            hideDirectUserObligationSummary,
            isAttention,
            replyRequired,
            ackStatus,
            isUserMessage,
        });
    }, [ackStatus, hideDirectUserObligationSummary, isAttention, isUserMessage, replyRequired]);

    const obligationSummary = useMemo(() => {
        return computeObligationSummary({
            hideDirectUserObligationSummary,
            obligationStatus: ev._obligation_status,
        });
    }, [ev._obligation_status, hideDirectUserObligationSummary]);

    const toLabel = useMemo(() => {
        return buildToLabel({
            hasDestination,
            dstGroupId,
            dstTo,
            groupLabelById,
            recipients,
            displayNameMap,
        });
    }, [displayNameMap, dstGroupId, dstTo, groupLabelById, hasDestination, recipients]);

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
            displayNameMap,
        });
    }, [displayNameMap, ev.by, senderActor, senderSnapshotTitle]);
    const senderAvatarUrl = useMemo(() => {
        return buildSenderAvatarUrl(blobGroupId, senderSnapshotAvatarPath) || String(senderActor?.avatar_url || "").trim();
    }, [blobGroupId, senderActor?.avatar_url, senderSnapshotAvatarPath]);
    const senderRuntime = senderSnapshotRuntime || String(senderActor?.runtime || "").trim();

    const readPreviewEntries = visibleReadStatusEntries.slice(0, 3);
    const readPreviewOverflow = Math.max(0, visibleReadStatusEntries.length - readPreviewEntries.length);
    const relayChipClass = "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] shadow-none hover:bg-[var(--glass-tab-bg-hover)]";

    useEffect(() => {
        if (!copiedMessageText) return undefined;
        const timer = window.setTimeout(() => {
            setCopiedMessageText(false);
        }, 1400);
        return () => window.clearTimeout(timer);
    }, [copiedMessageText]);

    const handleCopyMessageText = useCallback(async () => {
        const ok = await copyWithFeedback(copyableMessageText, {
            errorMessage: t("common:copyFailed", { defaultValue: "Copy failed" }),
        });
        if (ok) {
            setCopiedMessageText(true);
        }
    }, [copyWithFeedback, copyableMessageText, t]);

    return (
        <div
            className={classNames(
                "relative flex w-full min-w-0 gap-2 sm:gap-3 group",
                isUserMessage
                    ? "flex-col items-end sm:items-start sm:flex-row-reverse"
                    : "flex-col items-start sm:flex-row",
                isOptimistic ? "opacity-95" : ""
            )}
        >
            {/* Desktop Avatar (Hidden on mobile) */}
            <div className="relative hidden sm:block">
                <div
                    className={classNames(
                        "mt-1 h-8 w-8 flex-shrink-0",
                        collapseHeader ? "opacity-0 pointer-events-none" : "",
                        canShowAgentState && !collapseHeader ? "cursor-help" : ""
                    )}
                    ref={canShowAgentState && !collapseHeader ? setAgentStateReference : undefined}
                    onMouseEnter={canShowAgentState && !collapseHeader ? () => setIsAgentStateOpen(true) : undefined}
                    onMouseLeave={canShowAgentState && !collapseHeader ? () => setIsAgentStateOpen(false) : undefined}
                    onFocus={canShowAgentState && !collapseHeader ? () => setIsAgentStateOpen(true) : undefined}
                    onBlur={canShowAgentState && !collapseHeader ? () => setIsAgentStateOpen(false) : undefined}
                    tabIndex={canShowAgentState && !collapseHeader ? 0 : undefined}
                    aria-label={canShowAgentState && !collapseHeader ? t('agentStateTooltipLabel', { defaultValue: 'View agent state' }) : undefined}
                >
                    <ActorAvatar
                        avatarUrl={senderAvatarUrl || undefined}
                        runtime={senderRuntime || undefined}
                        title={senderDisplayName}
                        isUser={isUserMessage}
                        isDark={isDark}
                        accentRingClassName={senderAccent?.ring}
                    />
                </div>
            </div>
            <FloatingPortal>
                <AgentStateTooltip
                    isOpen={isAgentStateOpen && !collapseHeader}
                    canShow={canShowAgentState && !collapseHeader}
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

            {/* Message Content */}
            <div
                className={classNames(
                    "flex min-w-0 flex-col w-full md:w-auto",
                    isUserMessage ? "md:max-w-[min(42rem,78%)] xl:max-w-[min(44rem,72%)]" : "md:max-w-[min(48rem,86%)] xl:max-w-[min(52rem,80%)]",
                    isUserMessage ? "items-end" : "items-start"
                )}
            >
                {!collapseHeader ? (
                    <>
                        <MessageMetadataHeader
                            mobile={true}
                            isUserMessage={isUserMessage}
                            isDark={isDark}
                            senderAccentTextClass={senderAccent?.text}
                            senderDisplayName={senderDisplayName}
                            messageTimestamp={messageTimestamp}
                            fullMessageTimestamp={fullMessageTimestamp}
                            senderAvatarUrl={senderAvatarUrl || undefined}
                            senderRuntime={senderRuntime || undefined}
                            avatarRingClassName={senderAccent?.ring}
                        />

                        <MessageMetadataHeader
                            isUserMessage={isUserMessage}
                            isDark={isDark}
                            senderAccentTextClass={senderAccent?.text}
                            senderDisplayName={senderDisplayName}
                            messageTimestamp={messageTimestamp}
                            fullMessageTimestamp={fullMessageTimestamp}
                        />
                    </>
                ) : null}

                {/* Bubble wrapper (allows badge to overflow) */}
                <div
                    className={classNames(
                        "relative max-w-full min-w-0 md:w-auto",
                        isUserMessage ? "w-auto self-end" : "w-full"
                    )}
                    style={isAttention ? { minWidth: "min(8.5rem, 85vw)" } : undefined}
                >
                    {isAttention && (
                        <span
                            className={classNames(
                                "absolute -top-2 z-10 text-[10px] font-semibold px-2 py-0.5 rounded-full border shadow-sm",
                                isUserMessage ? "left-3" : "right-3",
                                "bg-amber-100 text-amber-800 dark:bg-amber-900/60 dark:text-amber-200 border-amber-300 dark:border-amber-700"
                            )}
                        >
                            {t('important')}
                        </span>
                    )}
                <div
                    className={classNames(
                        "inline-flex max-w-full flex-col px-4 py-3 text-sm leading-relaxed transition-[opacity,transform,box-shadow,background-color,border-color] duration-200 ease-out",
                        isStreaming ? "opacity-95 translate-y-0" : "opacity-100 translate-y-0",
                        bubbleMotionClass,
                        isUserMessage
                            ? "glass-bubble w-auto min-w-[min(18rem,70vw)] rounded-[22px] rounded-tr-md"
                            : "w-full rounded-[22px] rounded-tl-md border border-[var(--glass-border-subtle)] text-[var(--color-text-primary)] shadow-[0_10px_28px_rgba(15,23,42,0.06)]"
                        ,
                        isAttention ? "ring-1 ring-amber-400/40 dark:ring-amber-500/40" : ""
                        ,
                        isHighlighted ? "outline outline-2 outline-[rgb(35,36,37)]/16 outline-offset-2 dark:outline-white/18" : ""
                    )}
                >

                    <MessageBubbleBody
                        event={ev}
                        isUserMessage={isUserMessage}
                        isDark={isDark}
                        groupLabelById={groupLabelById}
                        toLabel={toLabel}
                        hasSource={hasSource}
                        srcGroupId={srcGroupId}
                        srcEventId={srcEventId}
                        hasDestination={hasDestination}
                        dstGroupId={dstGroupId}
                        dstTo={dstTo}
                        relayChipClass={relayChipClass}
                        quoteText={quoteText}
                        replyToEventId={replyToEventId}
                        presentationRefs={presentationRefs}
                        taskRefs={taskRefs}
                        taskById={taskById}
                        messageText={displayMessageText}
                        shouldRenderMarkdown={shouldRenderMarkdown}
                        blobAttachments={blobAttachments}
                        blobGroupId={blobGroupId}
                        stableMessageAttachmentKey={stableMessageAttachmentKey}
                        onOpenSource={onOpenSource}
                        onOpenPresentationRef={onOpenPresentationRef}
                        onOpenTaskRef={onOpenTaskRef}
                        onOpenReplyTarget={onOpenReplyTarget}
                    />
                </div>
                </div>

                <MessageFooter
                    readOnly={readOnly}
                    obligationSummary={obligationSummary}
                    ackSummary={ackSummary}
                    visibleReadStatusEntries={visibleReadStatusEntries}
                    readPreviewEntries={readPreviewEntries}
                    readPreviewOverflow={readPreviewOverflow}
                    displayNameMap={displayNameMap}
                    isDark={isDark}
                    replyRequired={replyRequired}
                    copiedMessageText={copiedMessageText}
                    copyableMessageText={copyableMessageText}
                    onCopyMessageText={() => void handleCopyMessageText()}
                    onShowRecipients={onShowRecipients}
                    onCopyLink={onCopyLink}
                    onRelay={onRelay}
                    onReply={onReply}
                    canReply={canReply}
                    eventId={typeof ev.id === "string" ? String(ev.id) : undefined}
                    event={ev}
                />
            </div>
        </div>
    );
}, (prevProps, nextProps) => {
    return (
        prevProps.event === nextProps.event &&
        prevProps.actors === nextProps.actors &&
        prevProps.displayNameMap === nextProps.displayNameMap &&
        prevProps.agentState === nextProps.agentState &&
        prevProps.taskById === nextProps.taskById &&
        prevProps.isDark === nextProps.isDark &&
        prevProps.groupId === nextProps.groupId &&
        prevProps.groupLabelById === nextProps.groupLabelById &&
        prevProps.isHighlighted === nextProps.isHighlighted &&
        prevProps.collapseHeader === nextProps.collapseHeader &&
        prevProps.onRelay === nextProps.onRelay &&
        prevProps.onOpenSource === nextProps.onOpenSource &&
        prevProps.onOpenPresentationRef === nextProps.onOpenPresentationRef &&
        prevProps.onOpenTaskRef === nextProps.onOpenTaskRef &&
        prevProps.onOpenReplyTarget === nextProps.onOpenReplyTarget
    );
});
