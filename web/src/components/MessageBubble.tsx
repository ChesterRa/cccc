import { memo, useCallback, useMemo, useState } from "react";
import { useTranslation } from 'react-i18next';

import {
    useFloating,
    autoUpdate,
    offset,
    flip,
    shift,
    useHover,
    useInteractions,
    useDismiss,
    FloatingPortal,
} from "@floating-ui/react";
import { LedgerEvent, Actor, PresenceAgent, getActorAccentColor, ChatMessageData, EventAttachment } from "../types";
import { formatFullTime, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { MarkdownRenderer } from "./MarkdownRenderer";
import { getRecipientDisplayName } from "../hooks/useActorDisplayName";
import { ImageIcon, FileIcon } from "./Icons";


function formatEventLine(ev: LedgerEvent): string {
    if (ev.kind === "chat.message" && ev.data && typeof ev.data === "object") {
        const msg = ev.data as ChatMessageData;
        return String(msg.text || "");
    }
    return "";
}

// Image preview component with loading state and error handling
function ImagePreview({
    href,
    alt,
    isUserMessage,
    isDark,
}: {
    href: string;
    alt: string;
    isUserMessage: boolean;
    isDark: boolean;
}) {
    const [loadError, setLoadError] = useState(false);
    const { t } = useTranslation('chat');

    if (loadError) {
        // Fallback to file download link on error
        return (
            <a
                href={href}
                className={classNames(
                    "inline-flex items-center gap-2 rounded px-2 py-1.5 text-xs transition-colors max-w-full",
                    isUserMessage
                        ? "bg-blue-700/50 hover:bg-blue-700 text-white border border-blue-500"
                        : isDark
                            ? "bg-slate-900/50 hover:bg-slate-900 text-slate-300 border border-slate-700"
                            : "bg-gray-50 hover:bg-gray-100 text-gray-700 border border-gray-200"
                )}
                title={t('download', { name: alt })}
                download
            >
                <ImageIcon size={14} className="opacity-70 flex-shrink-0" />
                <span className="truncate">{alt}</span>
            </a>
        );
    }

    return (
        <a
            href={href}
            target="_blank"
            rel="noopener noreferrer"
            className="block rounded-lg overflow-hidden max-w-full"
        >
            <img
                src={href}
                alt={alt}
                className="max-w-full max-h-64 sm:max-h-80 object-contain rounded-lg"
                loading="lazy"
                onError={() => setLoadError(true)}
            />
        </a>
    );
}

export interface MessageBubbleProps {
    event: LedgerEvent;
    actors: Actor[];
    displayNameMap: Map<string, string>;
    presenceAgent: PresenceAgent | null;
    isDark: boolean;
    readOnly?: boolean;
    groupId: string;
    groupLabelById: Record<string, string>;
    isHighlighted?: boolean;
    onReply: () => void;
    onShowRecipients: () => void;
    onAck?: (eventId: string) => void;
    onCopyLink?: (eventId: string) => void;
    onRelay?: (ev: LedgerEvent) => void;
    onOpenSource?: (srcGroupId: string, srcEventId: string) => void;
}

export const MessageBubble = memo(function MessageBubble({
    event: ev,
    actors,
    displayNameMap,
    presenceAgent,
    isDark,
    readOnly,
    groupId,
    groupLabelById,
    isHighlighted,
    onReply,
    onShowRecipients,
    onAck,
    onCopyLink,
    onRelay,
    onOpenSource,
}: MessageBubbleProps) {
    const isUserMessage = ev.by === "user";
    const senderAccent = !isUserMessage ? getActorAccentColor(String(ev.by || ""), isDark) : null;

    // Floating UI for Presence Tooltip - replaces manual coordinate calculation
    const [isPresenceOpen, setIsPresenceOpen] = useState(false);

    const canShowPresence = useMemo(() => {
        if (isUserMessage) return false;
        const id = String(ev.by || "");
        if (!id) return false;
        return true;
    }, [ev.by, isUserMessage]);

    const { refs, floatingStyles, context } = useFloating({
        open: isPresenceOpen && canShowPresence,
        onOpenChange: setIsPresenceOpen,
        placement: "bottom-start",
        middleware: [
            offset(10),
            flip({ fallbackPlacements: ["top-start", "bottom-end", "top-end"] }),
            shift({ padding: 10 }),
        ],
        whileElementsMounted: autoUpdate,
    });

    const setPresenceReference = useCallback(
        (node: HTMLElement | null) => {
            refs.setReference(node);
        },
        [refs]
    );

    const setPresenceFloating = useCallback(
        (node: HTMLElement | null) => {
            refs.setFloating(node);
        },
        [refs]
    );

    const hover = useHover(context, {
        delay: { open: 100, close: 150 },
        enabled: canShowPresence,
    });
    const dismiss = useDismiss(context);
    const { getReferenceProps, getFloatingProps } = useInteractions([hover, dismiss]);
    const { t } = useTranslation('chat');


    // Treat data as ChatMessageData.
    const msgData = ev.data as ChatMessageData | undefined;
    const quoteText = msgData?.quote_text;
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
    const rawAttachments: EventAttachment[] = Array.isArray(msgData?.attachments) ? msgData.attachments : [];
    const blobAttachments = rawAttachments
        .filter((a): a is EventAttachment => a != null && typeof a === "object")
        .map((a) => ({
            kind: String(a.kind || "file"),
            path: String(a.path || ""),
            title: String(a.title || ""),
            bytes: Number(a.bytes || 0),
            mime_type: String(a.mime_type || ""),
        }))
        .filter((a) => a.path.startsWith("state/blobs/"));

    // Use event's group_id for blob URLs (attachments are stored in the event's original group)
    const blobGroupId = String(ev.group_id || "").trim() || groupId;

    const readStatus = ev._read_status;
    const ackStatus = ev._ack_status;
    const recipients = msgData?.to;

    const visibleReadStatusEntries = useMemo(() => {
        if (!readStatus) return [];
        return actors
            .map((a) => String(a.id || ""))
            .filter((id) => id && Object.prototype.hasOwnProperty.call(readStatus, id))
            .map((id) => [id, !!readStatus[id]] as const);
    }, [actors, readStatus]);

    const ackSummary = useMemo(() => {
        if ((!isAttention && !replyRequired) || !ackStatus || typeof ackStatus !== "object") return null;
        const ids = Object.keys(ackStatus);
        if (ids.length === 0) return null;
        const done = ids.reduce((n, id) => n + (ackStatus[id] ? 1 : 0), 0);
        const needsUserAck =
            Object.prototype.hasOwnProperty.call(ackStatus, "user") && !ackStatus["user"] && !isUserMessage;
        return { done, total: ids.length, needsUserAck };
    }, [ackStatus, isAttention, isUserMessage, replyRequired]);

    const obligationSummary = useMemo(() => {
        const os = ev._obligation_status;
        if (!os || typeof os !== "object") return null;
        const ids = Object.keys(os);
        if (ids.length === 0) return null;

        const requiresReply = ids.some((id) => !!os[id]?.reply_required);
        if (requiresReply) {
            const done = ids.reduce((n, id) => n + (os[id]?.replied ? 1 : 0), 0);
            return { kind: "reply" as const, done, total: ids.length };
        }
        const done = ids.reduce((n, id) => n + (os[id]?.acked ? 1 : 0), 0);
        return { kind: "ack" as const, done, total: ids.length };
    }, [ev._obligation_status]);

    const toLabel = useMemo(() => {
        if (hasDestination) {
            const dstLabel = String(groupLabelById?.[dstGroupId] || "").trim() || dstGroupId;
            const dstToLabel = dstTo.length > 0 ? dstTo.join(", ") : "@all";
            return `group: ${dstLabel} · ${dstToLabel}`;
        }
        if (!recipients || recipients.length === 0) return "@all";
        return recipients
            .map(r => getRecipientDisplayName(r, displayNameMap))
            .join(", ");
    }, [displayNameMap, dstGroupId, dstTo, groupLabelById, hasDestination, recipients]);

    // Sender display name (use title if available)
    const senderDisplayName = useMemo(() => {
        const by = String(ev.by || "");
        if (!by || by === "user") return by;
        return displayNameMap.get(by) || by;
    }, [ev.by, displayNameMap]);

    const readPreviewEntries = visibleReadStatusEntries.slice(0, 3);
    const readPreviewOverflow = Math.max(0, visibleReadStatusEntries.length - readPreviewEntries.length);

    return (
        <div
            className={classNames(
                "flex gap-2 sm:gap-3 group",
                isUserMessage
                    ? "flex-col items-end sm:items-start sm:flex-row-reverse"
                    : "flex-col items-start sm:flex-row"
            )}
        >
            {/* Desktop Avatar (Hidden on mobile) */}
            <div
                className={classNames(
                    "hidden sm:flex flex-shrink-0 w-8 h-8 rounded-full items-center justify-center text-xs font-bold shadow-sm mt-1",
                    isUserMessage
                        ? "bg-gradient-to-br from-blue-500 to-blue-600 text-white"
                        : isDark
                            ? "bg-slate-700 text-slate-200"
                            : "bg-white border border-gray-200 text-gray-700",
                    !isUserMessage && senderAccent ? `ring-1 ring-inset ${senderAccent.ring}` : ""
                )}
                ref={setPresenceReference}
                {...getReferenceProps()}
            >
                {isUserMessage ? "U" : (senderDisplayName || "?")[0].toUpperCase()}
            </div>

            {/* Message Content */}
            <div
                className={classNames(
                    "flex flex-col w-full sm:w-auto sm:max-w-[75%] min-w-0",
                    isUserMessage ? "items-end" : "items-start"
                )}
            >
                {/* Mobile Header Row (Visible only on mobile) */}
                <div
                    className={classNames(
                        "flex items-center gap-2 mb-1 sm:hidden min-w-0",
                        isUserMessage ? "justify-end" : "justify-start"
                    )}
                >
                    <div
                        className={classNames(
                            "flex-shrink-0 w-6 h-6 rounded-full flex items-center justify-center text-[10px] font-bold shadow-sm",
                            isUserMessage
                                ? "bg-gradient-to-br from-blue-500 to-blue-600 text-white"
                                : isDark
                                    ? "bg-slate-700 text-slate-200"
                                    : "bg-white border border-gray-200 text-gray-700",
                            !isUserMessage && senderAccent ? `ring-1 ring-inset ${senderAccent.ring}` : ""
                        )}
                    >
                        {isUserMessage ? "U" : (senderDisplayName || "?")[0].toUpperCase()}
                    </div>
                    <span
                        className={classNames(
                            "text-xs font-medium flex-shrink-0",
                            isUserMessage
                                ? isDark
                                    ? "text-slate-300"
                                    : "text-gray-700"
                                : senderAccent
                                    ? senderAccent.text
                                    : isDark
                                        ? "text-slate-300"
                                        : "text-gray-700"
                        )}
                    >
                        {senderDisplayName}
                    </span>
                    <span className={`text-[10px] flex-shrink-0 ${isDark ? "text-slate-500" : "text-gray-400"}`}>
                        {formatTime(ev.ts)}
                    </span>
                    <span
                        className={classNames(
                            "text-[10px] min-w-0 truncate",
                            isDark ? "text-slate-500" : "text-gray-500"
                        )}
                        title={`to ${toLabel}`}
                    >
                        to {toLabel}
                    </span>
                </div>

                {/* Desktop Metadata Header (Hidden on mobile) */}
                <div className="hidden sm:flex items-center gap-2 mb-1 px-1 min-w-0">
                    <span
                        className={classNames(
                            "text-[11px] font-medium flex-shrink-0",
                            isUserMessage
                                ? isDark
                                    ? "text-slate-400"
                                    : "text-gray-500"
                                : senderAccent
                                    ? senderAccent.text
                                    : isDark
                                        ? "text-slate-400"
                                        : "text-gray-500"
                        )}
                    >
                        {senderDisplayName}
                    </span>
                    <span className={`text-[10px] flex-shrink-0 ${isDark ? "text-slate-600" : "text-gray-400"}`}>
                        {formatTime(ev.ts)}
                    </span>
                    <span className={classNames("text-[10px] min-w-0 truncate", isDark ? "text-slate-500" : "text-gray-500")} title={`to ${toLabel}`}>
                        to {toLabel}
                    </span>
                </div>

                {/* Bubble wrapper (allows badge to overflow) */}
                <div className="relative max-w-[85vw] sm:max-w-full min-w-0">
                    {isAttention && (
                        <span
                            className={classNames(
                                "absolute -top-2 z-10 text-[10px] font-semibold px-2 py-0.5 rounded-full border shadow-sm",
                                isUserMessage ? "left-3" : "right-3",
                                isDark ? "bg-amber-900/60 text-amber-200 border-amber-700/50" : "bg-amber-50 text-amber-700 border-amber-200"
                            )}
                        >
                            {t('important')}
                        </span>
                    )}
                <div
                    className={classNames(
                        "px-4 py-2.5 shadow-sm text-sm leading-relaxed overflow-hidden",
                        isUserMessage
                            ? "bg-blue-600 text-white rounded-2xl rounded-tr-none"
                            : isDark
                                ? "bg-slate-800 text-slate-200 border border-slate-700 rounded-2xl rounded-tl-none"
                                : "bg-white text-gray-800 border border-gray-200 rounded-2xl rounded-tl-none"
                        ,
                        isAttention ? (isDark ? "ring-1 ring-amber-500/30" : "ring-1 ring-amber-500/25") : ""
                        ,
                        isHighlighted
                            ? isDark
                                ? "outline outline-2 outline-sky-400/40 outline-offset-2"
                                : "outline outline-2 outline-sky-500/30 outline-offset-2"
                            : ""
                    )}
                >

                    {/* Cross-group relay provenance */}
                    {hasSource ? (
                        <button
                            type="button"
                            className={classNames(
                                "mb-2 inline-flex items-center gap-2 text-xs font-medium rounded-lg px-2 py-1 border",
                                isDark
                                    ? "border-white/10 bg-slate-900/40 text-slate-300 hover:bg-slate-900/60"
                                    : "border-black/10 bg-gray-50 text-gray-700 hover:bg-gray-100",
                                onOpenSource ? "cursor-pointer" : "cursor-default"
                            )}
                            onClick={() => onOpenSource?.(srcGroupId, srcEventId)}
                            disabled={!onOpenSource}
                            title={t('openOriginalMessage')}
                        >
                            <span className="opacity-70">↗</span>
                            <span className="truncate">
                                {t('relayedFrom', { groupId: srcGroupId, eventId: srcEventId.slice(0, 8) })}
                            </span>
                        </button>
                    ) : null}
                    {/* Outbound cross-group send record */}
                    {hasDestination ? (() => {
                        const dstLabel = String(groupLabelById?.[dstGroupId] || "").trim() || dstGroupId;
                        const dstToLabel = dstTo.length > 0 ? dstTo.join(", ") : "@all";
                        return (
                            <div
                                className={classNames(
                                    "mb-2 inline-flex items-center gap-2 text-xs font-medium rounded-lg px-2 py-1 border",
                                    isDark
                                        ? "border-white/10 bg-slate-900/40 text-slate-300"
                                        : "border-black/10 bg-gray-50 text-gray-700"
                                )}
                                title={t('sentTo', { label: dstGroupId, to: dstToLabel })}
                            >
                                <span className="opacity-70">↗</span>
                                <span className="truncate">
                                    {t('sentTo', { label: dstLabel, to: dstToLabel })}
                                </span>
                            </div>
                        );
                    })() : null}
                    {/* Reply Context */}
                    {quoteText && (
                        <div
                            className={`mb-2 text-xs border-l-2 pl-2 italic truncate opacity-80 ${isUserMessage ? "border-blue-400" : isDark ? "border-slate-600" : "border-gray-300"
                                }`}
                        >
                            "{quoteText}"
                        </div>
                    )}

                    {/* Text Content */}
                    <MarkdownRenderer
                        content={formatEventLine(ev)}
                        isDark={isDark}
                        invertText={isUserMessage}
                        className="break-words [overflow-wrap:anywhere] max-w-full"
                    />

                    {/* Attachments */}
                    {blobAttachments.length > 0 && blobGroupId && (() => {
                        const imageAttachments = blobAttachments.filter((a) =>
                            a.mime_type.startsWith("image/")
                        );
                        const fileAttachments = blobAttachments.filter((a) =>
                            !a.mime_type.startsWith("image/")
                        );
                        return (
                            <>
                                {/* Image previews */}
                                {imageAttachments.length > 0 && (
                                    <div className="mt-3 flex flex-wrap gap-2">
                                        {imageAttachments.map((a, i) => {
                                            const parts = a.path.split("/");
                                            const blobName = parts[parts.length - 1] || "";
                                            const href = `/api/v1/groups/${encodeURIComponent(blobGroupId)}/blobs/${encodeURIComponent(blobName)}`;
                                            const label = a.title || blobName;
                                            return (
                                                <ImagePreview
                                                    key={`img-${blobName}:${i}`}
                                                    href={href}
                                                    alt={label}
                                                    isUserMessage={isUserMessage}
                                                    isDark={isDark}
                                                />
                                            );
                                        })}
                                    </div>
                                )}
                                {/* File attachments */}
                                {fileAttachments.length > 0 && (
                                    <div className="mt-3 flex flex-wrap gap-2">
                                        {fileAttachments.map((a, i) => {
                                            const parts = a.path.split("/");
                                            const blobName = parts[parts.length - 1] || "";
                                            const href = `/api/v1/groups/${encodeURIComponent(blobGroupId)}/blobs/${encodeURIComponent(blobName)}`;
                                            const label = a.title || blobName || "file";
                                            return (
                                                <a
                                                    key={`file-${blobName}:${i}`}
                                                    href={href}
                                                    className={classNames(
                                                        "inline-flex items-center gap-2 rounded px-2 py-1.5 text-xs transition-colors max-w-full",
                                                        isUserMessage
                                                            ? "bg-blue-700/50 hover:bg-blue-700 text-white border border-blue-500"
                                                            : isDark
                                                                ? "bg-slate-900/50 hover:bg-slate-900 text-slate-300 border border-slate-700"
                                                                : "bg-gray-50 hover:bg-gray-100 text-gray-700 border border-gray-200"
                                                    )}
                                                    title={t('download', { name: label })}
                                                    download
                                                >
                                                    <FileIcon size={14} className="opacity-70 flex-shrink-0" />
                                                    <span className="truncate">{label}</span>
                                                </a>
                                            );
                                        })}
                                    </div>
                                )}
                            </>
                        );
                    })()}
                </div>
                </div>

                <div
                    className={classNames(
                        "flex items-center gap-3 mt-1 px-1 text-[10px] transition-opacity",
                        (obligationSummary || ackSummary || visibleReadStatusEntries.length > 0 || replyRequired) ? "justify-between" : "justify-end",
                        "opacity-70 group-hover:opacity-100",
                        isDark ? "text-slate-500" : "text-gray-500"
                    )}
                >
                    {obligationSummary ? (
                        readOnly ? (
                            <div className="flex items-center gap-2 min-w-0 rounded-lg px-2 py-1">
                                <span
                                    className={classNames(
                                        "text-[10px] font-semibold tracking-tight",
                                        obligationSummary.done >= obligationSummary.total
                                            ? isDark
                                                ? "text-emerald-400"
                                                : "text-emerald-600"
                                            : isDark
                                                ? "text-amber-400"
                                                : "text-amber-600"
                                    )}
                                >
                                    {obligationSummary.kind === "reply" ? t('reply') : t('ack')} {obligationSummary.done}/{obligationSummary.total}
                                </span>
                            </div>
                        ) : (
                            <button
                                type="button"
                                className={classNames(
                                    "touch-target-sm flex items-center gap-2 min-w-0 rounded-lg px-2 py-1",
                                    isDark ? "hover:bg-slate-800/60" : "hover:bg-gray-100"
                                )}
                                onClick={onShowRecipients}
                                aria-label={t('showObligationStatus')}
                            >
                                <span
                                    className={classNames(
                                        "text-[10px] font-semibold tracking-tight",
                                        obligationSummary.done >= obligationSummary.total
                                            ? isDark
                                                ? "text-emerald-400"
                                                : "text-emerald-600"
                                            : isDark
                                                ? "text-amber-400"
                                                : "text-amber-600"
                                    )}
                                >
                                    {obligationSummary.kind === "reply" ? t('reply') : t('ack')} {obligationSummary.done}/{obligationSummary.total}
                                </span>
                            </button>
                        )
                    ) : ackSummary ? (
                        readOnly ? (
                            <div className="flex items-center gap-2 min-w-0 rounded-lg px-2 py-1">
                                <span
                                    className={classNames(
                                        "text-[10px] font-semibold tracking-tight",
                                        ackSummary.done >= ackSummary.total
                                            ? isDark
                                                ? "text-emerald-400"
                                                : "text-emerald-600"
                                            : isDark
                                                ? "text-amber-400"
                                                : "text-amber-600"
                                    )}
                                >
                                    {t('ack')} {ackSummary.done}/{ackSummary.total}
                                </span>
                            </div>
                        ) : (
                            <button
                                type="button"
                                className={classNames(
                                    "touch-target-sm flex items-center gap-2 min-w-0 rounded-lg px-2 py-1",
                                    isDark ? "hover:bg-slate-800/60" : "hover:bg-gray-100"
                                )}
                                onClick={onShowRecipients}
                                aria-label={t('showAckStatus')}
                            >
                                <span
                                    className={classNames(
                                        "text-[10px] font-semibold tracking-tight",
                                        ackSummary.done >= ackSummary.total
                                            ? isDark
                                                ? "text-emerald-400"
                                                : "text-emerald-600"
                                            : isDark
                                                ? "text-amber-400"
                                                : "text-amber-600"
                                    )}
                                >
                                    {t('ack')} {ackSummary.done}/{ackSummary.total}
                                </span>
                            </button>
                        )
                    ) : visibleReadStatusEntries.length > 0 ? (
                        readOnly ? (
                            <div className="flex items-center gap-2 min-w-0 rounded-lg px-2 py-1">
                                <div className="flex items-center gap-2 min-w-0">
                                    {readPreviewEntries.map(([id, cleared]) => (
                                        <span key={id} className="inline-flex items-center gap-1 min-w-0">
                                            <span className="truncate max-w-[10ch]">{displayNameMap.get(id) || id}</span>
                                            <span
                                                className={classNames(
                                                    "text-[10px] font-semibold tracking-tight",
                                                    cleared
                                                        ? isDark
                                                            ? "text-emerald-400"
                                                            : "text-emerald-600"
                                                        : isDark
                                                            ? "text-slate-500"
                                                            : "text-gray-500"
                                                )}
                                                aria-label={cleared ? t('read') : t('pending')}
                                            >
                                                {cleared ? "✓✓" : "✓"}
                                            </span>
                                        </span>
                                    ))}
                                    {readPreviewOverflow > 0 && (
                                        <span className={classNames("text-[10px]", isDark ? "text-slate-500" : "text-gray-500")}>
                                            +{readPreviewOverflow}
                                        </span>
                                    )}
                                </div>
                            </div>
                        ) : (
                            <button
                                type="button"
                                className={classNames(
                                    "touch-target-sm flex items-center gap-2 min-w-0 rounded-lg px-2 py-1",
                                    isDark ? "hover:bg-slate-800/60" : "hover:bg-gray-100"
                                )}
                                onClick={onShowRecipients}
                                aria-label={t('showRecipientStatus')}
                            >
                                <div className="flex items-center gap-2 min-w-0">
                                    {readPreviewEntries.map(([id, cleared]) => (
                                        <span key={id} className="inline-flex items-center gap-1 min-w-0">
                                            <span className="truncate max-w-[10ch]">{displayNameMap.get(id) || id}</span>
                                            <span
                                                className={classNames(
                                                    "text-[10px] font-semibold tracking-tight",
                                                    cleared
                                                        ? isDark
                                                            ? "text-emerald-400"
                                                            : "text-emerald-600"
                                                        : isDark
                                                            ? "text-slate-500"
                                                            : "text-gray-500"
                                                )}
                                                aria-label={cleared ? t('read') : t('pending')}
                                            >
                                                {cleared ? "✓✓" : "✓"}
                                            </span>
                                        </span>
                                    ))}
                                    {readPreviewOverflow > 0 && (
                                        <span className={classNames("text-[10px]", isDark ? "text-slate-500" : "text-gray-500")}>
                                            +{readPreviewOverflow}
                                        </span>
                                    )}
                                </div>
                            </button>
                        )
                    ) : null}

                    {!obligationSummary && !ackSummary && replyRequired && (
                        <span
                            className={classNames(
                                "text-[10px] font-semibold tracking-tight",
                                isDark ? "text-violet-300" : "text-violet-700"
                            )}
                        >
                            {t('needReply')}
                        </span>
                    )}

                    {!readOnly && (
                      <div className="flex items-center gap-1.5 flex-wrap">
                        {ackSummary && ackSummary.needsUserAck && ev.id && !isUserMessage && !replyRequired && (
                            <button
                                type="button"
                                className={classNames(
                                    "touch-target-sm px-2.5 py-1 rounded-lg border text-[11px] font-semibold transition-colors",
                                    isDark
                                        ? "border-amber-700/60 bg-amber-900/30 text-amber-200 hover:bg-amber-900/50"
                                        : "border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-100"
                                )}
                                onClick={() => onAck?.(String(ev.id))}
                                aria-label={t('acknowledgeImportant')}
                                disabled={!onAck}
                                title={t('acknowledge')}
                            >
                                {t('acknowledge')}
                            </button>
                        )}
                        {ev.id && onCopyLink ? (
                            <button
                                type="button"
                                className={classNames(
                                    "touch-target-sm px-2 py-1 rounded-lg text-[11px] font-medium transition-colors",
                                    isDark ? "text-slate-400 hover:text-slate-200 hover:bg-white/5" : "text-gray-500 hover:text-gray-700 hover:bg-black/5"
                                )}
                                onClick={() => onCopyLink(String(ev.id))}
                                title={t('copyLink')}
                            >
                                {t('copyLink')}
                            </button>
                        ) : null}
                        {ev.id && onRelay ? (
                            <button
                                type="button"
                                className={classNames(
                                    "touch-target-sm px-2 py-1 rounded-lg text-[11px] font-medium transition-colors",
                                    isDark ? "text-slate-400 hover:text-slate-200 hover:bg-white/5" : "text-gray-500 hover:text-gray-700 hover:bg-black/5"
                                )}
                                onClick={() => onRelay(ev)}
                                title={t('relayToGroup')}
                            >
                                {t('relay')}
                            </button>
                        ) : null}
                        <button
                            type="button"
                            className={classNames(
                                "touch-target-sm px-2 py-1 rounded-lg text-[11px] font-medium transition-colors",
                                isDark ? "text-slate-400 hover:text-slate-200 hover:bg-white/5" : "text-gray-500 hover:text-gray-700 hover:bg-black/5"
                            )}
                            onClick={onReply}
                        >
                            {t('reply')}
                        </button>
                      </div>
                    )}
                </div>
            </div>

            {isPresenceOpen && canShowPresence && (
                <FloatingPortal>
                    <div
                        ref={setPresenceFloating}
                        style={floatingStyles}
                        {...getFloatingProps()}
                        className={classNames(
                            "z-tooltip w-[min(360px,calc(100vw-32px))] rounded-xl border shadow-2xl px-3 py-2",
                            isDark
                                ? "bg-slate-900/95 border-white/10 text-slate-200"
                                : "bg-white/95 border-black/10 text-gray-900"
                        )}
                        role="status"
                    >
                        <div className="flex items-center gap-2">
                            <div
                                className={classNames("text-xs font-semibold", isDark ? "text-slate-200" : "text-gray-900")}
                            >
                                {senderDisplayName}
                            </div>
                            {presenceAgent?.updated_at ? (
                                <div
                                    className={classNames(
                                        "ml-auto text-xs tabular-nums",
                                        isDark ? "text-slate-400" : "text-gray-500"
                                    )}
                                    title={formatFullTime(presenceAgent.updated_at)}
                                >
                                    {t('updated', { time: formatTime(presenceAgent.updated_at) })}
                                </div>
                            ) : null}
                        </div>
                        <div
                            className={classNames("mt-1 text-xs whitespace-pre-wrap", isDark ? "text-slate-300" : "text-gray-700")}
                        >
                            {presenceAgent?.status ? presenceAgent.status : t('noPresenceYet')}
                        </div>
                    </div>
                </FloatingPortal>
            )}
        </div>
    );
}, (prevProps, nextProps) => {
    return (
        prevProps.event === nextProps.event &&
        prevProps.actors === nextProps.actors &&
        prevProps.displayNameMap === nextProps.displayNameMap &&
        prevProps.presenceAgent === nextProps.presenceAgent &&
        prevProps.isDark === nextProps.isDark &&
        prevProps.groupId === nextProps.groupId &&
        prevProps.groupLabelById === nextProps.groupLabelById &&
        prevProps.isHighlighted === nextProps.isHighlighted &&
        prevProps.onRelay === nextProps.onRelay &&
        prevProps.onOpenSource === nextProps.onOpenSource
    );
});
