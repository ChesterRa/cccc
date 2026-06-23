// ChatComposer renders the chat message composer.
import type { CSSProperties, Dispatch, RefObject, SetStateAction } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Actor, GroupMeta, LedgerEvent, PresentationMessageRef, ReplyTarget } from "../../types";
import { classNames } from "../../utils/classNames";
import { AttachmentIcon, SendIcon, ChevronDownIcon, ReplyIcon, CloseIcon, AlertIcon, SparklesIcon, CopyIcon } from "../../components/Icons";
import { ScrollFade } from "../../components/ScrollFade";
import { getPresentationRefChipLabel } from "../../utils/presentationRefs";
import { useTranslation } from 'react-i18next';
import { useCopyFeedback } from "../../hooks/useCopyFeedback";
import { VoiceSecretaryComposerControl, type VoiceSecretaryCaptureMode } from "./VoiceSecretaryComposerControl";
import { SlashCommandMenu } from "./SlashCommandMenu";
import { useGroupStore } from "../../stores";
import { filterSlashCommands, getVisibleSlashCommandPage, type SlashCommandItem } from "../../utils/slashCommands";
import { getComposerActionVisibility, getComposerCanSend } from "./chatComposerActions";
import { ComposerFilePreview } from "./ComposerFilePreview";
import { getMentionMenuLeft, getMentionTriggerX } from "./mentionMenuPosition";
import { ChatMentionMenu } from "./ChatMentionMenu";
import { getComposerGroupMentionInsertToken, getGroupRouteDisplayName, resolveComposerHashRouting, type ComposerMentionKind, type ComposerMentionSuggestion } from "./chatMentionSuggestions";
import type { ComposerAgentMentionToken, ComposerGroupMentionToken } from "../../hooks/composerGroupMentions";
import {
  createComposerAgentMentionToken,
  createComposerGroupMentionToken,
  pruneComposerAgentMentionTokens,
  pruneComposerGroupMentionTokens,
  resolveControlledComposerMentionContext,
} from "../../hooks/composerGroupMentions";
import {
  composerTargetAllowsSuggestedUserMessage,
  consumeSuggestedUserMessage,
  latestSuggestedUserMessage,
  readConsumedSuggestedUserMessageIds,
} from "../../utils/suggestedUserMessage";
import { formatRecipientIdentifier } from "../../utils/recipientIdentifier";

const SLASH_COMMAND_PAGE_SIZE = 8;
const MENTION_MENU_DESKTOP_WIDTH = 320;

type RecipientPopoverTarget = {
  key: string;
  label: string;
  detail: string;
  identifier: string;
  idLabel?: string;
  idValue?: string;
};

function getAgentMentionDisplayToken(selected: ComposerMentionSuggestion): string {
  const label = String(selected.label || selected.value || "").trim();
  return label.startsWith("@") ? label : `@${label}`;
}

function cleanVoicePromptContextText(value: unknown, maxLen = 240): string {
  const text = String(value || "").replace(/\s+/g, " ").trim();
  if (text.length <= maxLen) return text;
  return `${text.slice(0, Math.max(1, maxLen - 1)).trimEnd()}…`;
}

function buildRecentChatExcerptForVoicePrompt(events: LedgerEvent[]): string {
  const rows: string[] = [];
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (String(event?.kind || "") !== "chat.message") continue;
    const data = event?.data && typeof event.data === "object" ? event.data as Record<string, unknown> : {};
    const text = cleanVoicePromptContextText(data.text, 220);
    if (!text) continue;
    const by = cleanVoicePromptContextText(event.by || data.by || "unknown", 40) || "unknown";
    rows.push(`${by}: ${text}`);
    if (rows.length >= 4) break;
  }
  return rows.reverse().join("\n").slice(0, 1000);
}

export interface ChatComposerProps {
  isDark: boolean;
  isSmallScreen: boolean;
  selectedGroupId: string;
  actors: Actor[];
  recipientActorsBusy?: boolean;
  destGroupId: string;
  setDestGroupId: (groupId: string) => void;
  composerGroupSettled: boolean;
  composerRouteGroups?: GroupMeta[];
  selectedGroupActorsHydrating?: boolean;
  destGroupScopeLabel?: string;
  busy: string;
  recentMessages?: LedgerEvent[];
  suggestionSourceMessages?: LedgerEvent[];

  // Reply
  replyTarget: ReplyTarget;
  onCancelReply: () => void;
  quotedPresentationRef: PresentationMessageRef | null;
  onClearQuotedPresentationRef: () => void;

  // Recipients
  toTokens: string[];
  onToggleRecipient: (token: string) => void;
  remoteGroups?: GroupMeta[];
  selectedRemoteGroupIds?: string[];
  onToggleRemoteGroup?: (groupId: string) => void;
  onClearRecipients: () => void;

  // Files
  composerFiles: File[];
  onRemoveComposerFile: (index: number) => void;
  appendComposerFiles: (files: File[]) => void;
  fileInputRef: RefObject<HTMLInputElement | null>;

  // Text input
  composerRef: RefObject<HTMLTextAreaElement | null>;
  composerText: string;
  setComposerText: Dispatch<SetStateAction<string>>;
  priority: "normal" | "attention";
  replyRequired: boolean;
  setPriority: (priority: "normal" | "attention") => void;
  setReplyRequired: (value: boolean) => void;
  onSendMessage: () => void;

  // Mention menu
  showMentionMenu: boolean;
  setShowMentionMenu: Dispatch<SetStateAction<boolean>>;
  mentionSuggestions: ComposerMentionSuggestion[];
  mentionSelectedIndex: number;
  setMentionSelectedIndex: Dispatch<SetStateAction<number>>;
  setMentionFilter: Dispatch<SetStateAction<string>>;
  setMentionKind: Dispatch<SetStateAction<ComposerMentionKind>>;
  setMentionActorScope: Dispatch<SetStateAction<"selected" | "destination">>;
  setMentionTargetGroupId: Dispatch<SetStateAction<string>>;
  onAppendRecipientToken: (token: string, label?: string) => void;
  composerGroupMentionTokens: ComposerGroupMentionToken[];
  setComposerGroupMentionTokens: Dispatch<SetStateAction<ComposerGroupMentionToken[]>>;
  composerAgentMentionTokens: ComposerAgentMentionToken[];
  setComposerAgentMentionTokens: Dispatch<SetStateAction<ComposerAgentMentionToken[]>>;
  slashCommands: SlashCommandItem[];
}


export function ChatComposer({
  isDark,
  isSmallScreen,
  selectedGroupId,
  actors,
  recipientActorsBusy,
  destGroupId,
  setDestGroupId,
  composerGroupSettled,
  composerRouteGroups = [],
  selectedGroupActorsHydrating,
  destGroupScopeLabel: _destGroupScopeLabel,
  busy,
  recentMessages = [],
  suggestionSourceMessages,
  replyTarget,
  onCancelReply,
  quotedPresentationRef,
  onClearQuotedPresentationRef,
  toTokens,
  onToggleRecipient,
  remoteGroups = [],
  selectedRemoteGroupIds = [],
  onToggleRemoteGroup,
  onClearRecipients,
  composerFiles,
  onRemoveComposerFile,
  appendComposerFiles,
  fileInputRef,
  composerRef,
  composerText,
  setComposerText,
  priority,
  replyRequired,
  setPriority,
  setReplyRequired,
  onSendMessage,
  showMentionMenu,
  setShowMentionMenu,
  mentionSuggestions,
  mentionSelectedIndex,
  setMentionSelectedIndex,
  setMentionFilter,
  setMentionKind,
  setMentionActorScope,
  setMentionTargetGroupId,
  onAppendRecipientToken,
  composerGroupMentionTokens,
  setComposerGroupMentionTokens,
  composerAgentMentionTokens,
  setComposerAgentMentionTokens,
  slashCommands,
}: ChatComposerProps) {
  const composerHeightRef = useRef(0);
  const isUserInputRef = useRef(false);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const [showSlashMenu, setShowSlashMenu] = useState(false);
  const [slashSelectedIndex, setSlashSelectedIndex] = useState(0);
  const [slashVisibleCount, setSlashVisibleCount] = useState(SLASH_COMMAND_PAGE_SIZE);
  const [voiceCaptureMode, setVoiceCaptureMode] = useState<VoiceSecretaryCaptureMode>("prompt");
  const [mentionMenuLeft, setMentionMenuLeft] = useState(8);
  const [composerScrollTop, setComposerScrollTop] = useState(0);
  const [sessionConsumedSuggestedUserMessageIds, setSessionConsumedSuggestedUserMessageIds] = useState<Set<string>>(
    () => new Set(),
  );
  const modeMenuRef = useRef<HTMLDivElement | null>(null);
  const recipientPopoverHideTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const [recipientPopoverTarget, setRecipientPopoverTarget] = useState<RecipientPopoverTarget | null>(null);
  const [recipientPopoverStyle, setRecipientPopoverStyle] = useState<CSSProperties | null>(null);
  const { t } = useTranslation('chat');
  const copyWithFeedback = useCopyFeedback();
  const groupSettings = useGroupStore((state) => state.groupSettings);
  const groups = useGroupStore((state) => state.groups);
  const routeGroups = composerRouteGroups.length > 0 ? composerRouteGroups : groups;
  const refreshSettings = useGroupStore((state) => state.refreshSettings);
  const selectedRemoteGroupSet = useMemo(
    () => new Set((selectedRemoteGroupIds || []).map((groupId) => String(groupId || "").trim()).filter(Boolean)),
    [selectedRemoteGroupIds],
  );
  const availableRemoteGroups = useMemo(
    () => (remoteGroups || []).filter((group) => String(group.group_id || "").trim() && group.federation_remote),
    [remoteGroups],
  );
  const visibleRecipientPopoverTarget = useMemo(() => {
    if (!recipientPopoverTarget) return null;
    if (!recipientPopoverTarget.key.startsWith("remote:")) return recipientPopoverTarget;
    const groupId = recipientPopoverTarget.key.slice("remote:".length);
    const stillAvailable = availableRemoteGroups.some((group) => String(group.group_id || "").trim() === groupId);
    return stillAvailable ? recipientPopoverTarget : null;
  }, [availableRemoteGroups, recipientPopoverTarget]);

  const readRootFontScale = () => {
    if (typeof document === "undefined") return 1;
    const rootFontSize = parseFloat(window.getComputedStyle(document.documentElement).fontSize);
    if (!Number.isFinite(rootFontSize) || rootFontSize <= 0) return 1;
    return rootFontSize / 16;
  };

  const [rootFontScale, setRootFontScale] = useState(readRootFontScale);
  const baseComposerHeight = (isSmallScreen ? 44 : 48) * rootFontScale;
  const maxComposerHeight = 128 * rootFontScale;
  const composerFontSize = (isSmallScreen ? 15 : 14) * rootFontScale;
  const composerLineHeight = (isSmallScreen ? 24 : 20) * rootFontScale;

  const resizeComposer = useCallback((node: HTMLTextAreaElement) => {
    node.style.height = "auto";
    const nextHeight = Math.min(Math.max(node.scrollHeight, baseComposerHeight), maxComposerHeight);
    node.style.height = `${nextHeight}px`;
    composerHeightRef.current = nextHeight;
  }, [baseComposerHeight, maxComposerHeight]);

  const updateMentionMenuPosition = useCallback((textToTrigger: string) => {
    const el = composerRef.current;
    if (!el || isSmallScreen) {
      setMentionMenuLeft(8);
      return;
    }
    setMentionMenuLeft(getMentionMenuLeft({
      triggerX: getMentionTriggerX(el, textToTrigger),
      containerWidth: el.clientWidth,
      menuWidth: MENTION_MENU_DESKTOP_WIDTH,
    }));
  }, [composerRef, isSmallScreen]);

  // Auto-adjust textarea height when composerText changes programmatically
  // (e.g. mention selection). Skips when handleChange already handled resize.
  useEffect(() => {
    if (isUserInputRef.current) {
      isUserInputRef.current = false;
      return;
    }
    const el = composerRef.current;
    if (!el) return;

    const rafId = requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        resizeComposer(el);
      });
    });

    return () => cancelAnimationFrame(rafId);
  }, [composerText, composerRef, resizeComposer]);

  useEffect(() => {
    const el = composerRef.current;
    if (!el) return;

    let rafId = 0;
    const observer = new MutationObserver(() => {
      setRootFontScale(readRootFontScale());
      cancelAnimationFrame(rafId);
      rafId = requestAnimationFrame(() => {
        resizeComposer(el);
      });
    });

    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
    return () => {
      cancelAnimationFrame(rafId);
      observer.disconnect();
    };
  }, [composerRef, resizeComposer]);

  useEffect(() => {
    if (!showModeMenu) return;

    const onPointerDown = (event: MouseEvent | TouchEvent) => {
      const node = modeMenuRef.current;
      if (!node) return;
      const target = event.target;
      if (target instanceof Node && !node.contains(target)) {
        setShowModeMenu(false);
      }
    };

    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("touchstart", onPointerDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("touchstart", onPointerDown);
    };
  }, [showModeMenu]);

  useEffect(() => () => {
    if (recipientPopoverHideTimerRef.current) {
      clearTimeout(recipientPopoverHideTimerRef.current);
    }
  }, []);

  useEffect(() => {
    if (!selectedGroupId || groupSettings) return;
    void refreshSettings(selectedGroupId);
  }, [groupSettings, refreshSettings, selectedGroupId]);

  const chipBaseClass =
    "flex h-6 flex-shrink-0 items-center justify-center whitespace-nowrap rounded-lg border px-2 text-[10px] font-medium leading-none transition-all sm:px-2.5 sm:text-[11px]";
  const chipActiveClass = isDark
    ? "border-white bg-white text-[rgb(20,20,22)] shadow-none"
    : "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white shadow-none";
  const chipInactiveClass = isDark
    ? "bg-white/[0.06] text-[var(--color-text-secondary)] border-white/[0.08] hover:bg-white/[0.1] hover:border-white/[0.14] hover:text-[var(--color-text-primary)]"
    : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)] border-transparent hover:bg-[rgb(237,237,237)] hover:border-black/5 hover:text-[rgb(20,20,22)]";
  const remoteChipActiveClass = isDark
    ? "border-sky-200 bg-sky-200 text-slate-950 shadow-none"
    : "border-sky-700 bg-sky-700 text-white shadow-none";
  const remoteChipInactiveClass = isDark
    ? "border-sky-300/20 bg-sky-400/10 text-sky-100 hover:border-sky-300/35 hover:bg-sky-400/15"
    : "border-sky-100 bg-sky-50 text-sky-950 hover:border-sky-200 hover:bg-sky-100";
  const cancelRecipientPopoverHide = useCallback(() => {
    if (recipientPopoverHideTimerRef.current) {
      clearTimeout(recipientPopoverHideTimerRef.current);
      recipientPopoverHideTimerRef.current = null;
    }
  }, []);
  const showRecipientPopover = useCallback((target: RecipientPopoverTarget, node: HTMLElement) => {
    cancelRecipientPopoverHide();
    const rect = node.getBoundingClientRect();
    const viewportWidth = typeof window === "undefined" ? 1024 : window.innerWidth;
    const tooltipWidth = Math.min(300, Math.max(220, viewportWidth - 16));
    const top = rect.bottom + 6;
    if (isSmallScreen) {
      setRecipientPopoverStyle({ top, left: 8, right: 8 });
    } else {
      setRecipientPopoverStyle({
        top,
        left: Math.min(Math.max(rect.left, 8), Math.max(8, viewportWidth - tooltipWidth - 8)),
        width: tooltipWidth,
      });
    }
    setRecipientPopoverTarget(target);
  }, [cancelRecipientPopoverHide, isSmallScreen]);
  const hideRecipientPopover = useCallback(() => {
    cancelRecipientPopoverHide();
    setRecipientPopoverTarget(null);
    setRecipientPopoverStyle(null);
  }, [cancelRecipientPopoverHide]);
  const scheduleRecipientPopoverHide = useCallback(() => {
    cancelRecipientPopoverHide();
    recipientPopoverHideTimerRef.current = setTimeout(() => {
      setRecipientPopoverTarget(null);
      setRecipientPopoverStyle(null);
      recipientPopoverHideTimerRef.current = null;
    }, 120);
  }, [cancelRecipientPopoverHide]);
  const getRemoteGroupAccessLabel = useCallback((_accessLevel: string) => {
    return t("remoteGroupMessagesOnly", { defaultValue: "Messages" });
  }, [t]);
  const copyRecipientIdentifier = useCallback(async (identifier: string) => {
    const text = String(identifier || "").trim();
    if (!text) return;
    await copyWithFeedback(text, {
      successMessage: t("recipientIdentifierCopied", { defaultValue: "Recipient identifier copied." }),
      errorMessage: t("common:copyFailed", { defaultValue: "Copy failed." }),
    });
  }, [copyWithFeedback, t]);
  const selectorPopoverTarget = useCallback((selector: string): RecipientPopoverTarget => ({
    key: `selector:${selector}`,
    label: selector,
    detail: t("recipientSelectorDetail", { defaultValue: "Local selector" }),
    identifier: formatRecipientIdentifier({ kind: "selector", selector }),
  }), [t]);
  const actorPopoverTarget = useCallback((actor: Actor): RecipientPopoverTarget => {
    const id = String(actor.id || "").trim();
    const label = String(actor.title || id || "actor").trim();
    const role = String(actor.role || "").trim();
    return {
      key: `actor:${id || label}`,
      label,
      detail: role
        ? t("recipientActorRoleDetail", { role, defaultValue: "Local {{role}} actor" })
        : t("recipientActorDetail", { defaultValue: "Local actor" }),
      identifier: formatRecipientIdentifier({ kind: "actor", label, id, role }),
      idLabel: t("recipientActorId", { defaultValue: "Actor ID" }),
      idValue: id,
    };
  }, [t]);
  const remoteGroupPopoverTarget = useCallback((group: GroupMeta): RecipientPopoverTarget => {
    const id = String(group.group_id || "").trim();
    const label = getGroupRouteDisplayName(group);
    const accessLevel = "messages";
    return {
      key: `remote:${id}`,
      label,
      detail: t("recipientRemoteGroupDetail", {
        access: getRemoteGroupAccessLabel(accessLevel),
        defaultValue: "Remote group · {{access}}",
      }),
      identifier: formatRecipientIdentifier({ kind: "remote_group", label, id, accessLevel }),
      idLabel: t("remoteGroupId", { defaultValue: "Remote group ID" }),
      idValue: id,
    };
  }, [getRemoteGroupAccessLabel, t]);

  // Get display name for reply target
  const replyByDisplayName = useMemo(() => {
    if (!replyTarget?.by) return "";
    if (replyTarget.by === "user") return "user";
    const actor = actors.find(a => a.id === replyTarget.by);
    return actor?.title || replyTarget.by;
  }, [replyTarget, actors]);
  const quotedPresentationRefLabel = useMemo(
    () => (quotedPresentationRef ? getPresentationRefChipLabel(quotedPresentationRef) : ""),
    [quotedPresentationRef],
  );
  const renderRecipientChipContent = useCallback((label: string) => (
    <span className="truncate">{label}</span>
  ), []);
  const slashSuggestions = useMemo(() => filterSlashCommands(slashCommands, composerText), [composerText, slashCommands]);
  const visibleSlashSuggestions = useMemo(
    () => getVisibleSlashCommandPage(slashSuggestions, slashVisibleCount),
    [slashSuggestions, slashVisibleCount],
  );
  const hasMoreSlashSuggestions = visibleSlashSuggestions.length < slashSuggestions.length;
  const liveGroupMentionTokens = useMemo(
    () => pruneComposerGroupMentionTokens({ text: composerText, tokens: composerGroupMentionTokens }),
    [composerGroupMentionTokens, composerText],
  );
  const liveAgentMentionTokens = useMemo(
    () => pruneComposerAgentMentionTokens({ text: composerText, tokens: composerAgentMentionTokens }),
    [composerAgentMentionTokens, composerText],
  );

  const mentionOverlay = useMemo(() => {
    const ranges = [
      ...liveGroupMentionTokens.map((token) => ({ ...token, kind: "group" as const })),
      ...liveAgentMentionTokens.map((token) => ({ ...token, kind: "agent" as const })),
    ];
    if (ranges.length === 0) return "";
    const sorted = ranges.sort((a, b) => a.start - b.start);
    let cursor = 0;
    const escapeHtml = (value: string) => value
      .replace(/&/g, "&amp;")
      .replace(/</g, "&lt;")
      .replace(/>/g, "&gt;");
    const parts: string[] = [];
    for (const token of sorted) {
      if (token.start < cursor) continue;
      parts.push(escapeHtml(composerText.slice(cursor, token.start)));
      const className = token.kind === "group"
        ? "rounded-md bg-sky-400/25 px-1 text-transparent ring-1 ring-inset ring-sky-300/60"
        : "rounded-md bg-violet-400/25 px-1 text-transparent ring-1 ring-inset ring-violet-300/60";
      parts.push(`<mark class="${className}">${escapeHtml(composerText.slice(token.start, token.end))}</mark>`);
      cursor = token.end;
    }
    parts.push(escapeHtml(composerText.slice(cursor)));
    return parts.join("");
  }, [composerText, liveAgentMentionTokens, liveGroupMentionTokens]);
  const consumedSuggestedUserMessageIds = readConsumedSuggestedUserMessageIds();
  for (const eventId of sessionConsumedSuggestedUserMessageIds) {
    consumedSuggestedUserMessageIds.add(eventId);
  }
  const suggestedUserMessage = latestSuggestedUserMessage(
    suggestionSourceMessages || recentMessages,
    consumedSuggestedUserMessageIds,
  );
  const canShowSuggestedUserMessageForTarget = composerTargetAllowsSuggestedUserMessage({
    selectedGroupId,
    destGroupId,
    composerGroupSettled,
  });
  const showSuggestedUserMessage = Boolean(
    suggestedUserMessage
    && canShowSuggestedUserMessageForTarget
    && !composerText.trim()
    && composerFiles.length === 0
    && busy !== "send",
  );
  const suggestedUserMessageHelpId = showSuggestedUserMessage
    ? `suggested-user-message-${suggestedUserMessage?.eventId || "current"}`
    : undefined;
  const suggestedUserMessageHintLabel = t("suggestedUserMessageHint", {
    defaultValue: "Suggested next message. Press Tab to use it.",
  });
  const suggestedUserMessageUseLabel = t("suggestedUserMessageUse", {
    defaultValue: "Use suggestion",
  });
  const markSuggestedUserMessageConsumed = useCallback(() => {
    const eventId = String(suggestedUserMessage?.eventId || "").trim();
    if (!eventId) return;
    consumeSuggestedUserMessage(eventId);
    setSessionConsumedSuggestedUserMessageIds((current) => {
      if (current.has(eventId)) return current;
      const next = new Set(current);
      next.add(eventId);
      return next;
    });
  }, [setSessionConsumedSuggestedUserMessageIds, suggestedUserMessage?.eventId]);
  const acceptSuggestedUserMessage = useCallback(() => {
    const text = String(suggestedUserMessage?.text || "").trim();
    if (!showSuggestedUserMessage || !text) return;
    markSuggestedUserMessageConsumed();
    setComposerText(text);
    requestAnimationFrame(() => {
      const textarea = composerRef.current;
      if (!textarea) return;
      textarea.focus();
      textarea.setSelectionRange(text.length, text.length);
    });
  }, [composerRef, markSuggestedUserMessageConsumed, setComposerText, showSuggestedUserMessage, suggestedUserMessage?.text]);

  // Handle pasted files (clipboard items).
  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const dt = e.clipboardData;
    if (!dt) return;

    const files: File[] = [];
    try {
      const items = Array.from(dt.items || []);
      for (const it of items) {
        if (!it || it.kind !== "file") continue;
        const f = it.getAsFile();
        if (f) files.push(f);
      }
    } catch {
      // ignore
    }
    if (files.length === 0) {
      try {
        files.push(...Array.from(dt.files || []));
      } catch {
        // ignore
      }
    }

    if (files.length === 0) return;
    if (!selectedGroupId) return;

    // De-duplicate within a single paste.
    const seen = new Set<string>();
    const unique: File[] = [];
    for (const f of files) {
      const key = `${f.name}:${f.size}:${f.type}`;
      if (seen.has(key)) continue;
      seen.add(key);
      unique.push(f);
    }
    if (unique.length === 0) return;

    e.preventDefault();
    appendComposerFiles(unique);
  };

  // Handle text changes.
  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    if (showSuggestedUserMessage && val.trim()) {
      markSuggestedUserMessageConsumed();
    }
    isUserInputRef.current = true;
    setComposerText(val);
    setComposerGroupMentionTokens((tokens) => pruneComposerGroupMentionTokens({ text: val, tokens }));
    setComposerAgentMentionTokens((tokens) => pruneComposerAgentMentionTokens({ text: val, tokens }));
    const target = e.target;
    // Use requestAnimationFrame to avoid forced reflow during layout.
    requestAnimationFrame(() => {
      resizeComposer(target);
    });

    const slashModeActive = val === val.trimStart() && val.startsWith("/") && !val.slice(1).includes(" ");
    // A user's `#<group>` token is a local-group agent delegation hint, not a
    // cross-group route: keep the destination pinned to the local group so the
    // message is never sent directly to the referenced group. The token itself
    // stays in the composer text as delegation context for the local agent.
    const hashRouting = resolveComposerHashRouting({
      text: val,
      selectedGroupId,
      groups: routeGroups,
    });
    if (hashRouting.destGroupId !== destGroupId) {
      setDestGroupId(hashRouting.destGroupId);
    }

    if (slashModeActive) {
      const nextSuggestions = filterSlashCommands(slashCommands, val);
      setShowSlashMenu(nextSuggestions.length > 0 || val === "/");
      setSlashSelectedIndex(0);
      setSlashVisibleCount(SLASH_COMMAND_PAGE_SIZE);
      setShowMentionMenu(false);
      setMentionActorScope("selected");
      setMentionTargetGroupId("");
      setMentionFilter("");
      return;
    }
    setShowSlashMenu(false);
    setSlashVisibleCount(SLASH_COMMAND_PAGE_SIZE);

    // Detect @ agent mentions for the recipient helper menu.
    const lastAt = val.lastIndexOf("@");
    if (lastAt >= 0) {
      const afterAt = val.slice(lastAt + 1);
      if (
        (lastAt === 0 || val[lastAt - 1] === " " || val[lastAt - 1] === "\n") &&
        !afterAt.includes(" ") &&
        !afterAt.includes("\n")
      ) {
        // Context-sensitive scope: a valid, same-segment `#group` before this
        // `@` targets that group's actors; otherwise it's a local mention. A
        // bare `@` (no in-segment `#group`) always stays local.
        const mentionCtx = resolveControlledComposerMentionContext({
          text: val,
          atIndex: lastAt,
          tokens: composerGroupMentionTokens,
        });
        setMentionKind("agent");
        setMentionActorScope(mentionCtx.scope);
        setMentionTargetGroupId(mentionCtx.mentionTargetGroupId);
        setMentionFilter(afterAt);
        setShowMentionMenu(true);
        setMentionSelectedIndex(0);
        requestAnimationFrame(() => updateMentionMenuPosition(val.slice(0, lastAt + 1)));
        return;
      }
    }

    // Detect # group route mentions for the destination group helper menu.
    const lastHash = val.lastIndexOf("#");
    if (lastHash >= 0) {
      const afterHash = val.slice(lastHash + 1);
      if (
        (lastHash === 0 || val[lastHash - 1] === " " || val[lastHash - 1] === "\n") &&
        !afterHash.includes(" ") &&
        !afterHash.includes("\n")
      ) {
        setMentionKind("group");
        setMentionActorScope("selected");
        setMentionTargetGroupId("");
        setMentionFilter(afterHash);
        setShowMentionMenu(true);
        setMentionSelectedIndex(0);
        requestAnimationFrame(() => updateMentionMenuPosition(val.slice(0, lastHash + 1)));
      } else {
        setShowMentionMenu(false);
        setMentionActorScope("selected");
        setMentionTargetGroupId("");
        setMentionFilter("");
      }
    } else {
      setShowMentionMenu(false);
      setMentionActorScope("selected");
      setMentionTargetGroupId("");
      setMentionFilter("");
    }
  };

  // Handle keyboard shortcuts and mention navigation.
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (showSlashMenu && visibleSlashSuggestions.length > 0) {
      const maxIndex = visibleSlashSuggestions.length - 1;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSlashSelectedIndex((prev) => {
          const next = prev >= maxIndex ? 0 : prev + 1;
          if (hasMoreSlashSuggestions && next === maxIndex) {
            setSlashVisibleCount((count) => Math.min(count + SLASH_COMMAND_PAGE_SIZE, slashSuggestions.length));
          }
          return next;
        });
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setSlashSelectedIndex((prev) => (prev <= 0 ? maxIndex : prev - 1));
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        selectSlashCommand(visibleSlashSuggestions[slashSelectedIndex]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setShowSlashMenu(false);
        setSlashSelectedIndex(0);
        return;
      }
    }
    if (showMentionMenu && mentionSuggestions.length > 0) {
      const maxIndex = mentionSuggestions.length - 1;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setMentionSelectedIndex((prev) => (prev >= maxIndex ? 0 : prev + 1));
        return;
      }
      if (e.key === "ArrowUp") {
        e.preventDefault();
        setMentionSelectedIndex((prev) => (prev <= 0 ? maxIndex : prev - 1));
        return;
      }
      if (e.key === "Enter" || e.key === "Tab") {
        e.preventDefault();
        selectMention(mentionSuggestions[mentionSelectedIndex]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        setShowMentionMenu(false);
        setMentionSelectedIndex(0);
        return;
      }
    }
    if (
      showSuggestedUserMessage
      && e.key === "Tab"
      && !e.shiftKey
      && !e.ctrlKey
      && !e.metaKey
      && !e.altKey
    ) {
      e.preventDefault();
      acceptSuggestedUserMessage();
      return;
    }
    if (showSuggestedUserMessage && e.key === "Escape") {
      e.preventDefault();
      markSuggestedUserMessageConsumed();
      return;
    }
    if (e.key === "Enter" && !showMentionMenu) {
      if (showSlashMenu) return;
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        if (canSend) onSendMessage();
      }
    } else if (e.key === "Escape") {
      setShowMentionMenu(false);
      setShowSlashMenu(false);
      setShowModeMenu(false);
      onCancelReply();
    }
  };

  // Select an agent from @ or a destination group from #.
  const selectMention = (selected: ComposerMentionSuggestion | undefined) => {
    if (!selected) return;
    if (selected.kind === "agent") {
      const lastAt = composerText.lastIndexOf("@");
      const mentionScope = lastAt >= 0
        ? resolveControlledComposerMentionContext({ text: composerText, atIndex: lastAt, tokens: composerGroupMentionTokens }).scope
        : "selected";
      if (lastAt >= 0) {
        const before = composerText.slice(0, lastAt);
        const tokenText = getAgentMentionDisplayToken(selected);
        const nextText = before + tokenText + " ";
        setComposerText(nextText);
        const token = createComposerAgentMentionToken({
          actorId: selected.value,
          token: tokenText,
          start: before.length,
          scope: mentionScope,
        });
        if (token) {
          setComposerAgentMentionTokens((tokens) => pruneComposerAgentMentionTokens({ text: before, tokens }).concat([token]));
        }
      }
      // Destination-scope `@` names a target-group agent for the delegation
      // relay (extracted from text at send time) — it must NOT become a local
      // recipient. Only local (selected-scope) mentions go into `to`.
      if (mentionScope !== "destination" && !toTokens.includes(selected.value)) {
        onAppendRecipientToken(selected.value, selected.label);
      }
      setShowMentionMenu(false);
      setMentionSelectedIndex(0);
      return;
    }

    const lastHash = composerText.lastIndexOf("#");
    if (lastHash >= 0) {
      const before = composerText.slice(0, lastHash);
      const tokenText = getComposerGroupMentionInsertToken(selected);
      setComposerText(before + tokenText + " ");
      const token = createComposerGroupMentionToken({
        groupId: selected.value,
        token: tokenText,
        start: before.length,
      });
      if (token) {
        setComposerGroupMentionTokens((tokens) => pruneComposerGroupMentionTokens({ text: before, tokens }).concat([token]));
      }
    }
    // Inserting a `#<group>` token is a local-group delegation hint only — it
    // must NOT set a cross-group destination. The destination stays local.
    setShowMentionMenu(false);
    setMentionSelectedIndex(0);
  };

  const selectSlashCommand = (selected: SlashCommandItem | undefined) => {
    if (!selected) return;
    setComposerText(`/${selected.name} `);
    setShowSlashMenu(false);
    setSlashSelectedIndex(0);
    requestAnimationFrame(() => composerRef.current?.focus());
  };

  const canSend = getComposerCanSend({
    composerText,
    composerFilesCount: composerFiles.length,
    recipientResolutionBusy: selectedGroupActorsHydrating || recipientActorsBusy,
  });
  const isAttention = priority === "attention";
  const isCrossGroup = !!destGroupId && destGroupId !== selectedGroupId;
  const actorChipDisabled = !selectedGroupId || busy === "send" || !!selectedGroupActorsHydrating;
  const actionVisibility = getComposerActionVisibility(isSmallScreen);

  type MessageMode = "normal" | "attention" | "task";
  const modeOptions: Array<{ key: MessageMode; label: string; description: string }> = [
    { key: "normal", label: t('modeNormal'), description: t('modeNormalDesc') },
    { key: "attention", label: t('modeImportant'), description: t('modeImportantDesc') },
    { key: "task", label: t('modeNeedReply'), description: t('modeNeedReplyDesc') },
  ];

  const messageMode: MessageMode = replyRequired
    ? "task"
    : isAttention
      ? "attention"
      : "normal";
  const setMessageMode = (mode: MessageMode) => {
    if (mode === "normal") {
      setPriority("normal");
      setReplyRequired(false);
      return;
    }
    if (mode === "attention") {
      setPriority("attention");
      setReplyRequired(false);
      return;
    }
    setPriority("normal");
    setReplyRequired(true);
  };
  const activeMode = modeOptions.find((opt) => opt.key === messageMode) || modeOptions[0];
  const modeNotice = messageMode === "task"
    ? t('modeNoticeNeedReply')
    : messageMode === "attention"
      ? t('modeNoticeImportant')
      : "";

  const recentChatExcerpt = useMemo(() => buildRecentChatExcerptForVoicePrompt(recentMessages), [recentMessages]);

  const composerAssistantContext = useMemo<Record<string, unknown>>(() => ({
    recipients: toTokens,
    message_mode: messageMode,
    priority,
    reply_required: replyRequired,
    reply_target: replyTarget
      ? `${replyTarget.by || "unknown"}: ${String(replyTarget.text || "").slice(0, 240)}`
      : "",
    quoted_reference: quotedPresentationRef ? getPresentationRefChipLabel(quotedPresentationRef) : "",
    recent_chat_excerpt: recentChatExcerpt,
  }), [messageMode, priority, quotedPresentationRef, recentChatExcerpt, replyRequired, replyTarget, toTokens]);

  const fillPromptDraftFromSpeech = useCallback((draft: string, opts?: { mode?: "replace" | "append" }) => {
    const text = String(draft || "").trim();
    if (!text) return;
    setComposerText((current) => {
      const existing = String(current || "");
      if (opts?.mode === "replace" || !existing.trim()) return text;
      return `${existing.replace(/\s+$/g, "")}\n\n${text}`;
    });
    requestAnimationFrame(() => {
      const textarea = composerRef.current;
      if (!textarea) return;
      textarea.focus();
      const end = textarea.value.length;
      textarea.setSelectionRange(end, end);
    });
  }, [composerRef, setComposerText]);

  const fileDisabledReason = (() => {
    if (!selectedGroupId) return t('selectGroupFirst');
    if (busy === "send") return t('busy');
    if (isCrossGroup) return t('crossGroupAttachment');
    return t('attachFile');
  })();
  const sendShortcutLabel = useMemo(() => {
    if (typeof navigator !== "undefined" && /Mac|iPhone|iPad|iPod/i.test(navigator.platform || "")) {
      return "⌘+Enter";
    }
    return "Ctrl+Enter";
  }, []);
  const sendButtonTitle = t("sendMessageWithShortcut", {
    shortcut: sendShortcutLabel,
    defaultValue: "Send message ({{shortcut}})",
  });
  const composerPlaceholder = showSuggestedUserMessage
    ? ""
    : isSmallScreen ? t('messagePlaceholder') : t('messagePlaceholderDesktop');

  return (
    <footer
      className={classNames(
        "relative z-40 flex-shrink-0 border-t px-2 py-1.5 safe-area-bottom-compact transition-colors sm:px-2.5 sm:py-2",
        "border-[var(--glass-border)] bg-[var(--glass-panel-bg)] backdrop-blur-md"
      )}
    >
        {/* Reply indicator */}
        {replyTarget && (
          <div className={classNames(
            "mb-2.5 flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[11px]",
            isDark
              ? "border-white/[0.06] bg-white/[0.035] text-[var(--color-text-tertiary)]"
              : "border-black/[0.05] bg-black/[0.025] text-gray-500"
          )}>
            <ReplyIcon size={12} className="flex-shrink-0 opacity-45" />
            <span className="min-w-0 flex-1 truncate">
              <span className="mr-1 opacity-55">{t('replyingTo')}</span>
              <span className={classNames("font-medium", isDark ? "text-slate-300/90" : "text-gray-700")}>
                {replyByDisplayName}
              </span>
              <span className="mx-1 opacity-40">"</span>
              <span className="opacity-75">{replyTarget.text}</span>
              <span className="opacity-40">"</span>
            </span>
            <button
              className={classNames(
                "rounded-full p-1 transition-colors",
                isDark
                  ? "text-[var(--color-text-tertiary)] hover:bg-white/[0.08] hover:text-[var(--color-text-primary)]"
                  : "text-gray-400 hover:bg-black/[0.06] hover:text-gray-600"
              )}
              onClick={onCancelReply}
              title={t('cancelReply')}
              aria-label={t('cancelReply')}
            >
              <CloseIcon size={14} />
            </button>
          </div>
        )}

        {quotedPresentationRef && (
          <div
            className={classNames(
              "mb-2.5 flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[11px]",
              isDark
                ? "border-cyan-400/12 bg-cyan-500/6 text-[var(--color-text-tertiary)]"
                : "border-cyan-200/70 bg-cyan-50/70 text-gray-600",
            )}
          >
            <span className={classNames("flex-shrink-0 font-medium", isDark ? "text-cyan-100/90" : "text-cyan-700")}>
              {t("presentationQuotedViewLabel", { defaultValue: "Quoted view" })}
            </span>
            <span className="min-w-0 flex-1 truncate opacity-80" title={quotedPresentationRef.title || quotedPresentationRefLabel}>
              {quotedPresentationRefLabel}
            </span>
            <button
              className={classNames(
                "rounded-full p-1 transition-colors",
                isDark
                  ? "text-[var(--color-text-tertiary)] hover:bg-white/[0.08] hover:text-[var(--color-text-primary)]"
                  : "text-gray-400 hover:bg-black/[0.06] hover:text-gray-600",
              )}
              onClick={onClearQuotedPresentationRef}
              title={t("presentationRemoveQuotedView", { defaultValue: "Remove quoted view" })}
              aria-label={t("presentationRemoveQuotedView", { defaultValue: "Remove quoted view" })}
            >
              <CloseIcon size={14} />
            </button>
          </div>
        )}

        {/* File list */}
        {composerFiles.length > 0 && (
          <div className="mb-3 flex flex-wrap gap-2 animate-in fade-in slide-in-from-bottom-2 duration-300">
            {composerFiles.map((f, idx) => (
              <ComposerFilePreview
                key={`${f.name}:${idx}`}
                file={f}
                onRemove={() => onRemoveComposerFile(idx)}
                removeLabel={t('removeAttachment', { name: f.name })}
              />
            ))}
          </div>
        )}

        {modeNotice ? (
          <div
            className={classNames(
              "mb-3 rounded-lg border px-3 py-1.5 text-[11px] leading-5",
              messageMode === "task"
                ? isDark
                  ? "border-violet-500/30 bg-violet-500/10 text-violet-200"
                  : "border-violet-200 bg-violet-50 text-violet-700"
                : isDark
                  ? "border-amber-500/30 bg-amber-500/10 text-amber-200"
                  : "border-amber-200 bg-amber-50 text-amber-700"
            )}
            role="status"
            aria-live="polite"
          >
            {modeNotice}
          </div>
        ) : null}

        <input
          ref={fileInputRef as RefObject<HTMLInputElement>}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            const files = Array.from(e.target.files || []);
            if (files.length > 0) appendComposerFiles(files);
            e.target.value = "";
          }}
        />

        {/* Integrated composer */}
        <div className="flex flex-col">
          <div
            className="relative flex min-w-0 flex-1 flex-col"
          >
            {/* Row 1 — Recipients */}
            <div
              className={classNames(
                "relative flex items-center gap-1.5 border-b px-2.5 py-1",
                isDark ? "border-white/[0.04]" : "border-black/[0.04]",
              )}
            >
              <span className={classNames("flex-shrink-0 text-[10px] font-medium tracking-[0.08em]", isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-400")}>
                {t('to', 'To')}
              </span>

              <ScrollFade
                className="min-w-0 flex-1"
                innerClassName="w-full max-w-full"
                fadeWidth={20}
              >
                <div
                  className={classNames(
                    "flex min-w-max items-center gap-1 transition-opacity",
                  )}
                >
                  <div
                    className={classNames(
                      "flex items-center gap-1 transition-opacity",
                      selectedGroupActorsHydrating ? "opacity-50 pointer-events-none" : "",
                    )}
                  >
                    {["@all", "@foreman", "@peers"].map((tok) => {
                      const active = toTokens.includes(tok);
                      const popoverTarget = selectorPopoverTarget(tok);
                      return (
                        <button
                          key={tok}
                          className={classNames(
                            chipBaseClass,
                            active
                              ? chipActiveClass
                              : chipInactiveClass,
                          )}
                          onClick={() => onToggleRecipient(tok)}
                          onMouseEnter={(event) => showRecipientPopover(popoverTarget, event.currentTarget)}
                          onMouseLeave={scheduleRecipientPopoverHide}
                          onFocus={(event) => showRecipientPopover(popoverTarget, event.currentTarget)}
                          onBlur={scheduleRecipientPopoverHide}
                          disabled={!selectedGroupId || busy === "send"}
                          aria-pressed={active}
                        >
                          {renderRecipientChipContent(tok)}
                        </button>
                      );
                    })}
                    {actors.map((actor) => {
                      const id = String(actor.id || "");
                      if (!id) return null;
                      const active = toTokens.includes(id);
                      const popoverTarget = actorPopoverTarget(actor);
                      return (
                        <button
                          key={id}
                          className={classNames(
                            chipBaseClass,
                            active
                              ? chipActiveClass
                              : chipInactiveClass,
                          )}
                          onClick={() => onToggleRecipient(id)}
                          onMouseEnter={(event) => showRecipientPopover(popoverTarget, event.currentTarget)}
                          onMouseLeave={scheduleRecipientPopoverHide}
                          onFocus={(event) => showRecipientPopover(popoverTarget, event.currentTarget)}
                          onBlur={scheduleRecipientPopoverHide}
                          disabled={actorChipDisabled}
                          aria-pressed={active}
                        >
                          {renderRecipientChipContent(actor.title || id)}
                        </button>
                      );
                    })}
                  </div>
                  {availableRemoteGroups.length > 0 ? (
                    <div className={classNames("mx-1 h-4 w-px flex-shrink-0", isDark ? "bg-white/10" : "bg-black/10")} aria-hidden="true" />
                  ) : null}
                  {availableRemoteGroups.map((group) => {
                    const groupId = String(group.group_id || "").trim();
                    const label = getGroupRouteDisplayName(group);
                    const active = selectedRemoteGroupSet.has(groupId);
                    const accessLevel = "messages";
                    const accessLabel = getRemoteGroupAccessLabel(accessLevel);
                    const popoverTarget = remoteGroupPopoverTarget(group);
                    const title = [
                      label,
                      t("remoteGroupSendsToForeman", { defaultValue: "Sends to the remote foreman." }),
                      groupId,
                    ].filter(Boolean).join(" · ");
                    return (
                      <div
                        key={groupId}
                        className={classNames(
                          "flex h-6 flex-shrink-0 items-center overflow-hidden whitespace-nowrap rounded-lg border text-[10px] font-medium leading-none transition-all sm:text-[11px]",
                          "max-w-[9rem] sm:max-w-[12rem]",
                          active ? remoteChipActiveClass : remoteChipInactiveClass,
                        )}
                        onMouseEnter={(event) => showRecipientPopover(popoverTarget, event.currentTarget as HTMLElement)}
                        onMouseLeave={scheduleRecipientPopoverHide}
                        data-remote-group-id={groupId}
                        data-remote-group-access={accessLabel}
                        title={title}
                      >
                        <button
                          type="button"
                          className="flex h-full min-w-0 flex-1 items-center justify-center px-2 sm:px-2.5"
                          onFocus={(event) => showRecipientPopover(popoverTarget, event.currentTarget)}
                          onBlur={scheduleRecipientPopoverHide}
                          onClick={() => onToggleRemoteGroup?.(groupId)}
                          disabled={!selectedGroupId || busy === "send" || !onToggleRemoteGroup}
                          aria-pressed={active}
                          aria-label={t("remoteGroupChipLabel", { name: label, defaultValue: "Remote group {{name}}" })}
                        >
                          <span className="truncate">{label}</span>
                        </button>
                      </div>
                    );
                  })}
                </div>
              </ScrollFade>

              {typeof document !== "undefined" && visibleRecipientPopoverTarget && recipientPopoverStyle ? createPortal((
                <div
                  className={classNames(
                    "fixed z-[1000] rounded-lg border px-3 py-2 text-xs shadow-xl backdrop-blur-xl",
                    isDark
                      ? "border-white/12 bg-[rgb(24,25,27)] text-slate-100"
                      : "border-black/10 bg-white text-gray-900",
                  )}
                  style={recipientPopoverStyle}
                  role="dialog"
                  aria-label={t("recipientDetails", { name: visibleRecipientPopoverTarget.label, defaultValue: "Recipient details for {{name}}" })}
                  onMouseEnter={cancelRecipientPopoverHide}
                  onMouseLeave={scheduleRecipientPopoverHide}
                >
                  <div className="flex min-w-0 items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold">{visibleRecipientPopoverTarget.label}</div>
                      <div className={classNames("mt-1 text-[11px]", isDark ? "text-slate-400" : "text-gray-500")}>
                        {visibleRecipientPopoverTarget.detail}
                      </div>
                    </div>
                    <button
                      type="button"
                      className={classNames(
                        "inline-flex h-7 flex-shrink-0 items-center gap-1 rounded-md px-2 text-[11px] font-semibold transition-colors",
                        isDark ? "text-slate-300 hover:bg-white/10 hover:text-white" : "text-gray-500 hover:bg-black/5 hover:text-gray-800",
                      )}
                      onClick={() => {
                        void copyRecipientIdentifier(visibleRecipientPopoverTarget.identifier);
                        hideRecipientPopover();
                      }}
                      aria-label={t("copyRecipientIdentifier", { defaultValue: "Copy identifier" })}
                      title={t("copyRecipientIdentifier", { defaultValue: "Copy identifier" })}
                    >
                      <CopyIcon size={13} aria-hidden="true" />
                      <span>{t("copyRecipientIdentifier", { defaultValue: "Copy identifier" })}</span>
                    </button>
                  </div>
                  <div className="mt-2 flex min-w-0 items-center gap-2">
                    <code className={classNames("min-w-0 flex-1 truncate rounded-md px-2 py-1 font-mono text-[11px]", isDark ? "bg-white/[0.08] text-slate-200" : "bg-gray-100 text-gray-800")}>
                      {visibleRecipientPopoverTarget.identifier}
                    </code>
                  </div>
                  {visibleRecipientPopoverTarget.idValue ? (
                    <div className={classNames("mt-1 text-[11px]", isDark ? "text-slate-400" : "text-gray-500")}>
                      <span className="font-semibold uppercase tracking-wide">{visibleRecipientPopoverTarget.idLabel}</span>
                      <span className="ml-2 font-mono">{visibleRecipientPopoverTarget.idValue}</span>
                    </div>
                  ) : null}
                </div>
              ), document.body) : null}

              {(toTokens.length > 0 || selectedRemoteGroupIds.length > 0) && (
                <button
                  className={classNames(
                    "flex-shrink-0 h-7 w-7 rounded-full flex items-center justify-center transition-colors opacity-50 hover:opacity-100",
                    isDark ? "text-[var(--color-text-tertiary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]" : "text-gray-400 hover:bg-black/5 hover:text-gray-700",
                  )}
                  onClick={onClearRecipients}
                  disabled={busy === "send"}
                  aria-label={t('clearRecipients')}
                  title={t('clearRecipients')}
                >
                  <CloseIcon size={12} />
                </button>
              )}
            </div>

            {/* Row 2 — Textarea */}
            <div className="relative min-w-0 flex-1">
              {mentionOverlay ? (
                <div
                  className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre-wrap break-words border-0 px-4 py-3 text-transparent"
                  style={{
                    minHeight: `${Math.max(baseComposerHeight + 6, 52)}px`,
                    maxHeight: `${maxComposerHeight}px`,
                    fontSize: `${composerFontSize}px`,
                    lineHeight: `${composerLineHeight}px`,
                  }}
                  aria-hidden="true"
                >
                  <div
                    style={{
                      transform: `translateY(-${composerScrollTop}px)`,
                    }}
                    dangerouslySetInnerHTML={{ __html: mentionOverlay }}
                  />
                </div>
              ) : null}
              <textarea
                ref={composerRef as RefObject<HTMLTextAreaElement>}
                className={classNames(
                  "relative w-full bg-transparent border-0 py-3 resize-none overflow-y-auto scrollbar-hide focus:outline-none focus:ring-0 text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)]",
                  showSuggestedUserMessage ? "pl-11 pr-4" : "px-4",
                )}
                style={{
                  minHeight: `${Math.max(baseComposerHeight + 6, 52)}px`,
                  maxHeight: `${maxComposerHeight}px`,
                  fontSize: `${composerFontSize}px`,
                  lineHeight: `${composerLineHeight}px`,
                }}
                placeholder={composerPlaceholder}
                rows={1}
                value={composerText}
                onPaste={handlePaste}
                onChange={handleChange}
                onKeyDown={handleKeyDown}
                onScroll={(event) => setComposerScrollTop(event.currentTarget.scrollTop)}
                onBlur={() => setTimeout(() => setShowMentionMenu(false), 150)}
                aria-label={t('messageInput')}
                aria-describedby={suggestedUserMessageHelpId}
              />
              {showSuggestedUserMessage && suggestedUserMessage ? (
                <button
                  type="button"
                  className={classNames(
                    "absolute left-3 top-3 z-10 flex h-6 w-6 items-center justify-center rounded-md transition-colors",
                    isDark
                      ? "text-white/45 hover:bg-white/10 hover:text-white/75"
                      : "text-gray-400 hover:bg-black/[0.06] hover:text-gray-600",
                  )}
                  onClick={acceptSuggestedUserMessage}
                  aria-label={suggestedUserMessageUseLabel}
                  title={suggestedUserMessageUseLabel}
                >
                  <SparklesIcon size={14} aria-hidden="true" />
                </button>
              ) : null}
              {showSuggestedUserMessage && suggestedUserMessage ? (
                <div
                  className={classNames(
                    "pointer-events-none absolute inset-x-0 top-0 overflow-hidden py-3 pl-11 pr-4 whitespace-pre-wrap",
                    isDark ? "text-white/22" : "text-gray-400/80",
                  )}
                  style={{
                    maxHeight: `${maxComposerHeight}px`,
                    fontSize: `${composerFontSize}px`,
                    lineHeight: `${composerLineHeight}px`,
                  }}
                  aria-hidden="true"
                >
                  {suggestedUserMessage.text}
                </div>
              ) : null}
              {showSuggestedUserMessage && suggestedUserMessageHelpId ? (
                <span id={suggestedUserMessageHelpId} className="sr-only">
                  {suggestedUserMessageHintLabel}
                </span>
              ) : null}

              {/* Mention menu */}
              {showMentionMenu && mentionSuggestions.length > 0 && (
                <ChatMentionMenu
                  isDark={isDark}
                  isSmallScreen={isSmallScreen}
                  items={mentionSuggestions}
                  left={mentionMenuLeft}
                  selectedIndex={mentionSelectedIndex}
                  onSelect={(item) => {
                    selectMention(item);
                    composerRef.current?.focus();
                  }}
                  onHover={setMentionSelectedIndex}
                />
              )}

              {showSlashMenu && visibleSlashSuggestions.length > 0 && (
                <SlashCommandMenu
                  isDark={isDark}
                  suggestions={visibleSlashSuggestions}
                  selectedIndex={Math.min(slashSelectedIndex, visibleSlashSuggestions.length - 1)}
                  hasMore={hasMoreSlashSuggestions}
                  loadMoreLabel={t("slashCommandLoadMore", { defaultValue: "Scroll for more" })}
                  onSelect={selectSlashCommand}
                  onHover={setSlashSelectedIndex}
                  onLoadMore={() => {
                    setSlashVisibleCount((count) => Math.min(count + SLASH_COMMAND_PAGE_SIZE, slashSuggestions.length));
                  }}
                />
              )}
            </div>
            {/* Row 3 — Action bar */}
            <div
              className={classNames(
                "grid grid-cols-[2.75rem_minmax(0,1fr)_2.75rem] items-center gap-2 px-2 pb-2 pt-1 sm:flex sm:justify-between",
              )}
            >
              <div className="contents sm:flex sm:items-center sm:gap-1.5">
                <button
                  className={classNames(
                    "glass-btn flex h-11 w-11 items-center justify-center rounded-lg text-[var(--color-text-secondary)] transition-colors disabled:cursor-not-allowed disabled:text-[var(--color-text-tertiary)] disabled:opacity-60 sm:h-9 sm:w-9",
                    busy !== "send" && selectedGroupId && !isCrossGroup
                      ? isDark ? "hover:bg-white/10 hover:text-[var(--color-text-primary)]" : "hover:bg-black/5 hover:text-gray-800"
                      : "",
                  )}
                  onClick={() => fileInputRef.current?.click()}
                  disabled={!selectedGroupId || busy === "send" || isCrossGroup}
                  aria-label={t('attachFile')}
                  title={fileDisabledReason}
                >
                  <AttachmentIcon size={18} />
                </button>

                <div className="min-w-0 sm:min-w-max">
                <VoiceSecretaryComposerControl
                  isDark={isDark}
                  selectedGroupId={selectedGroupId}
                  busy={busy}
                  disabled={!selectedGroupId || busy === "send" || !composerGroupSettled}
                  variant="assistantRow"
                  captureMode={voiceCaptureMode}
                  onCaptureModeChange={setVoiceCaptureMode}
                  composerText={composerText}
                  composerContext={composerAssistantContext}
                  onPromptDraft={fillPromptDraftFromSpeech}
                />
                </div>
              </div>

              <div className="contents sm:flex sm:items-center sm:gap-1.5">
                {actionVisibility.showMessageModeSelector ? (
                  <div ref={modeMenuRef} className="relative z-20">
                    <button
                      type="button"
                      className={classNames(
                        "inline-flex h-9 items-center gap-1.5 rounded-lg px-2.5 text-[11px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-60",
                        busy === "send" || !selectedGroupId
                          ? isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-400"
                          : messageMode === "task"
                            ? isDark
                              ? "bg-violet-500/18 text-violet-200 hover:bg-violet-500/26"
                              : "bg-violet-100 text-violet-700 hover:bg-violet-200"
                            : messageMode === "attention"
                              ? isDark
                                ? "bg-amber-500/18 text-amber-200 hover:bg-amber-500/26"
                                : "bg-amber-100 text-amber-700 hover:bg-amber-200"
                              : isDark
                                ? "text-slate-200 hover:bg-white/10"
                                : "text-gray-700 hover:bg-black/5",
                      )}
                      disabled={busy === "send" || !selectedGroupId}
                      onClick={() => setShowModeMenu((v) => !v)}
                      aria-label={t('messageType')}
                      aria-haspopup="menu"
                      aria-expanded={showModeMenu}
                      title={t('messageMode', { mode: activeMode.label })}
                    >
                      {messageMode === "task" ? (
                        <ReplyIcon size={13} />
                      ) : messageMode === "attention" ? (
                        <AlertIcon size={13} />
                      ) : (
                        <span className="text-[11px] font-black italic leading-none">N</span>
                      )}
                      <span className="hidden sm:inline">{activeMode.label}</span>
                      <ChevronDownIcon size={12} className="opacity-70" />
                    </button>

                    {showModeMenu && (
                      <div
                        className={classNames(
                          "glass-panel absolute bottom-full right-0 mb-2 z-40 w-56 sm:w-64 rounded-2xl border p-1.5 shadow-2xl pointer-events-auto",
                        )}
                        role="menu"
                        aria-label={t('messageTypeOptions')}
                      >
                        {modeOptions.map((opt) => {
                          const active = messageMode === opt.key;
                          return (
                            <button
                              key={opt.key}
                              type="button"
                              className={classNames(
                                "w-full rounded-xl px-3 py-2.5 text-left flex items-center gap-2.5 transition-colors",
                                active
                                  ? isDark
                                    ? "bg-white/10"
                                    : "bg-black/5"
                                  : isDark
                                    ? "hover:bg-white/5"
                                    : "hover:bg-black/5",
                              )}
                              role="menuitemradio"
                              aria-checked={active}
                              onClick={() => {
                                setMessageMode(opt.key);
                                setShowModeMenu(false);
                              }}
                            >
                              <span
                                className={classNames(
                                  "w-6 h-6 rounded-md flex items-center justify-center flex-shrink-0",
                                  opt.key === "task"
                                    ? isDark
                                      ? "bg-violet-500/25 text-violet-200"
                                      : "bg-violet-100 text-violet-700"
                                    : opt.key === "attention"
                                      ? isDark
                                        ? "bg-amber-500/25 text-amber-200"
                                        : "bg-amber-100 text-amber-700"
                                      : isDark
                                        ? "bg-slate-700 text-slate-200"
                                        : "bg-gray-100 text-gray-700",
                                )}
                              >
                                {opt.key === "task" ? (
                                  <ReplyIcon size={13} />
                                ) : opt.key === "attention" ? (
                                  <AlertIcon size={13} />
                                ) : (
                                  <span className="text-[11px] font-black italic leading-none">N</span>
                                )}
                              </span>
                              <span className="min-w-0 flex-1">
                                <span className={classNames("block text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>
                                  {opt.label}
                                </span>
                                <span className={classNames("block text-[11px]", isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-500")}>
                                  {opt.description}
                                </span>
                              </span>
                              {active && <span className={classNames("text-xs font-semibold", isDark ? "text-emerald-300" : "text-emerald-600")}>✓</span>}
                            </button>
                          );
                        })}
                      </div>
                    )}
                  </div>
                ) : null}

                <button
                  className={classNames(
                    "flex h-11 w-11 items-center justify-center rounded-lg font-semibold transition-[background-color,box-shadow,transform] duration-150 disabled:cursor-not-allowed sm:h-9 sm:w-[5.5rem]",
                    busy === "send" || !canSend
                      ? isDark ? "bg-white/[0.06] text-[var(--color-text-tertiary)]" : "bg-gray-100 text-gray-400"
                      : "bg-[var(--color-accent-primary)] text-[var(--color-text-inverse)] shadow-[var(--glass-accent-shadow)] hover:brightness-110 active:scale-[0.97]",
                  )}
                  onClick={onSendMessage}
                  disabled={busy === "send" || !canSend}
                  aria-label={t('sendMessage')}
                  title={sendButtonTitle}
                >
                  {busy === "send" ? (
                    <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                  ) : (
                    <>
                      <SendIcon size={16} className="sm:hidden" />
                      <span className="hidden sm:inline">{t('send')}</span>
                    </>
                  )}
                </button>
              </div>
            </div>
          </div>

        </div>
    </footer>
  );
}
