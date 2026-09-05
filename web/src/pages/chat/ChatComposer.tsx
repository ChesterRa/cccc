// ChatComposer renders the chat message composer.
import type { Dispatch, RefObject, SetStateAction } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Actor,
  GroupMeta,
  LedgerEvent,
  PresentationMessageRef,
  ReplyTarget,
  VoiceDocumentMessageRef,
} from "../../types";
import { classNames } from "../../utils/classNames";
import {
  AttachmentIcon,
  SendIcon,
  ChevronDownIcon,
  ReplyIcon,
  CloseIcon,
  InboxIcon,
  SparklesIcon,
} from "../../components/Icons";
import { getPresentationRefChipLabel } from "../../utils/presentationRefs";
import { getVoiceDocumentRefLabel } from "../../utils/voiceDocumentRefs";
import { useTranslation } from "react-i18next";
import {
  LazyVoiceSecretaryComposerControl,
  type VoiceSecretaryCaptureMode,
} from "./LazyVoiceSecretaryComposerControl";
import { SlashCommandMenu } from "./SlashCommandMenu";
import { useGroupStore } from "../../stores";
import {
  filterSlashCommands,
  getVisibleSlashCommandPage,
  type SlashCommandItem,
} from "../../utils/slashCommands";
import { getComposerCanSend, hasConcreteReplyRecipients } from "./chatComposerActions";
import { ComposerFilePreview } from "./ComposerFilePreview";
import { getMentionMenuLeft, getMentionTriggerX } from "./mentionMenuPosition";
import { ChatMentionMenu } from "./ChatMentionMenu";
import {
  getComposerGroupMentionInsertToken,
  resolveComposerHashRouting,
  type ComposerMentionKind,
  type ComposerMentionSuggestion,
} from "./chatMentionSuggestions";
import type {
  ComposerAgentMentionToken,
  ComposerGroupMentionToken,
} from "../../hooks/composerGroupMentions";
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
import { ComposerRecipientsRow } from "./ComposerRecipientsRow";
import { useComposerTextareaAutoResize } from "./useComposerTextareaAutoResize";
import {
  buildComposerHistoryEntries,
  canStartComposerHistory,
  getComposerHistoryText,
  moveComposerHistory,
  startComposerHistory,
  type ComposerHistorySession,
} from "./chatComposerHistory";
import {
  normalizeReplyMessageMode,
  useComposerStore,
  type ComposerMessageMode,
} from "../../stores/useComposerStore";

const SLASH_COMMAND_PAGE_SIZE = 8;
const MENTION_MENU_DESKTOP_WIDTH = 320;

function getAgentMentionDisplayToken(selected: ComposerMentionSuggestion): string {
  const label = String(selected.label || selected.value || "").trim();
  return label.startsWith("@") ? label : `@${label}`;
}

function cleanVoicePromptContextText(value: unknown, maxLen = 240): string {
  const text = String(value || "")
    .replace(/\s+/g, " ")
    .trim();
  if (text.length <= maxLen) return text;
  return `${text.slice(0, Math.max(1, maxLen - 1)).trimEnd()}…`;
}

function buildRecentChatExcerptForVoicePrompt(events: LedgerEvent[]): string {
  const rows: string[] = [];
  for (let index = events.length - 1; index >= 0; index -= 1) {
    const event = events[index];
    if (String(event?.kind || "") !== "chat.message") continue;
    const data =
      event?.data && typeof event.data === "object" ? (event.data as Record<string, unknown>) : {};
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
  quotedVoiceDocumentRef: VoiceDocumentMessageRef | null;
  onQuoteVoiceDocumentRef: (ref: VoiceDocumentMessageRef) => void;
  onClearQuotedVoiceDocumentRef: () => void;

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
  setComposerText: Dispatch<SetStateAction<string>>;
  messageMode: ComposerMessageMode;
  setMessageMode: (mode: ComposerMessageMode) => void;
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
  quotedVoiceDocumentRef,
  onQuoteVoiceDocumentRef,
  onClearQuotedVoiceDocumentRef,
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
  setComposerText,
  messageMode,
  setMessageMode,
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
  composerGroupMentionTokens,
  setComposerGroupMentionTokens,
  composerAgentMentionTokens,
  setComposerAgentMentionTokens,
  slashCommands,
}: ChatComposerProps) {
  // The draft text is owned here, not by ChatTab: only the composer needs a
  // re-render per keystroke.
  const composerText = useComposerStore((s) => s.composerText);
  const composerHistoryRef = useRef<ComposerHistorySession | null>(null);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const [showSlashMenu, setShowSlashMenu] = useState(false);
  const [slashSelectedIndex, setSlashSelectedIndex] = useState(0);
  const [slashVisibleCount, setSlashVisibleCount] = useState(SLASH_COMMAND_PAGE_SIZE);
  const [voiceCaptureMode, setVoiceCaptureMode] = useState<VoiceSecretaryCaptureMode>("prompt");
  const [mentionMenuLeft, setMentionMenuLeft] = useState(8);
  const [composerScrollTop, setComposerScrollTop] = useState(0);
  const [sessionConsumedSuggestedUserMessageIds, setSessionConsumedSuggestedUserMessageIds] =
    useState<Set<string>>(() => new Set());
  const modeMenuRef = useRef<HTMLDivElement | null>(null);
  const { t } = useTranslation("chat");
  const groupSettings = useGroupStore((state) => state.groupSettings);
  const groups = useGroupStore((state) => state.groups);
  const routeGroups = composerRouteGroups.length > 0 ? composerRouteGroups : groups;
  const refreshSettings = useGroupStore((state) => state.refreshSettings);
  const readRootFontScale = () => {
    if (typeof document === "undefined") return 1;
    const rootFontSize = parseFloat(window.getComputedStyle(document.documentElement).fontSize);
    if (!Number.isFinite(rootFontSize) || rootFontSize <= 0) return 1;
    return rootFontSize / 16;
  };

  const [rootFontScale, setRootFontScale] = useState(readRootFontScale);
  const baseComposerHeight = (isSmallScreen ? 44 : 48) * rootFontScale;
  const desktopComposerHeight = 64 * rootFontScale;
  const minComposerHeight = isSmallScreen
    ? Math.max(baseComposerHeight + 6, 52)
    : desktopComposerHeight;
  const maxComposerHeight = isSmallScreen ? 128 * rootFontScale : desktopComposerHeight;
  const composerFontSize = (isSmallScreen ? 15 : 14) * rootFontScale;
  const composerLineHeight = (isSmallScreen ? 24 : 20) * rootFontScale;

  useComposerTextareaAutoResize({
    composerRef,
    value: composerText,
    minHeight: minComposerHeight,
    maxHeight: maxComposerHeight,
  });

  const exitComposerHistory = useCallback(() => {
    composerHistoryRef.current = null;
  }, []);

  const applyComposerHistoryText = useCallback(
    (text: string) => {
      // History recalls text only. Structured mention tokens encode routing context
      // from an older draft and must never be revived implicitly.
      setComposerGroupMentionTokens([]);
      setComposerAgentMentionTokens([]);
      setComposerText(text);
      requestAnimationFrame(() => {
        const textarea = composerRef.current;
        if (!textarea) return;
        textarea.focus();
        const end = textarea.value.length;
        textarea.setSelectionRange(end, end);
        textarea.scrollTop = textarea.scrollHeight;
        setComposerScrollTop(textarea.scrollTop);
      });
    },
    [composerRef, setComposerAgentMentionTokens, setComposerGroupMentionTokens, setComposerText],
  );

  useEffect(() => {
    exitComposerHistory();
  }, [exitComposerHistory, selectedGroupId]);

  useEffect(() => {
    const session = composerHistoryRef.current;
    if (!session) return;
    if (session.groupId !== selectedGroupId || composerText !== getComposerHistoryText(session)) {
      exitComposerHistory();
    }
  }, [composerText, exitComposerHistory, selectedGroupId]);

  const updateMentionMenuPosition = useCallback(
    (textToTrigger: string) => {
      const el = composerRef.current;
      if (!el || isSmallScreen) {
        setMentionMenuLeft(8);
        return;
      }
      setMentionMenuLeft(
        getMentionMenuLeft({
          triggerX: getMentionTriggerX(el, textToTrigger),
          containerWidth: el.clientWidth,
          menuWidth: MENTION_MENU_DESKTOP_WIDTH,
        }),
      );
    },
    [composerRef, isSmallScreen],
  );

  useEffect(() => {
    const observer = new MutationObserver(() => {
      setRootFontScale(readRootFontScale());
    });

    observer.observe(document.documentElement, { attributes: true, attributeFilter: ["style"] });
    return () => observer.disconnect();
  }, []);

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

  useEffect(() => {
    if (!selectedGroupId || groupSettings) return;
    void refreshSettings(selectedGroupId);
  }, [groupSettings, refreshSettings, selectedGroupId]);

  // Get display name for reply target
  const replyByDisplayName = useMemo(() => {
    if (!replyTarget?.by) return "";
    if (replyTarget.by === "user") return "user";
    const actor = actors.find((a) => a.id === replyTarget.by);
    return actor?.title || replyTarget.by;
  }, [replyTarget, actors]);
  const quotedPresentationRefLabel = useMemo(
    () => (quotedPresentationRef ? getPresentationRefChipLabel(quotedPresentationRef) : ""),
    [quotedPresentationRef],
  );
  const quotedVoiceDocumentRefLabel = useMemo(
    () => (quotedVoiceDocumentRef ? getVoiceDocumentRefLabel(quotedVoiceDocumentRef) : ""),
    [quotedVoiceDocumentRef],
  );
  const slashSuggestions = useMemo(
    () => filterSlashCommands(slashCommands, composerText),
    [composerText, slashCommands],
  );
  const visibleSlashSuggestions = useMemo(
    () => getVisibleSlashCommandPage(slashSuggestions, slashVisibleCount),
    [slashSuggestions, slashVisibleCount],
  );
  const hasMoreSlashSuggestions = visibleSlashSuggestions.length < slashSuggestions.length;
  const liveGroupMentionTokens = useMemo(
    () =>
      pruneComposerGroupMentionTokens({ text: composerText, tokens: composerGroupMentionTokens }),
    [composerGroupMentionTokens, composerText],
  );
  const liveAgentMentionTokens = useMemo(
    () =>
      pruneComposerAgentMentionTokens({ text: composerText, tokens: composerAgentMentionTokens }),
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
    const escapeHtml = (value: string) =>
      value.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    const parts: string[] = [];
    for (const token of sorted) {
      if (token.start < cursor) continue;
      parts.push(escapeHtml(composerText.slice(cursor, token.start)));
      const className =
        token.kind === "group"
          ? "rounded-md bg-sky-400/25 text-transparent ring-1 ring-inset ring-sky-300/60"
          : "rounded-md bg-violet-400/25 text-transparent ring-1 ring-inset ring-violet-300/60";
      parts.push(
        `<mark class="${className}">${escapeHtml(composerText.slice(token.start, token.end))}</mark>`,
      );
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
    suggestedUserMessage &&
    canShowSuggestedUserMessageForTarget &&
    !composerText.trim() &&
    composerFiles.length === 0 &&
    busy !== "send",
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
    exitComposerHistory();
    markSuggestedUserMessageConsumed();
    setComposerText(text);
    requestAnimationFrame(() => {
      const textarea = composerRef.current;
      if (!textarea) return;
      textarea.focus();
      textarea.setSelectionRange(text.length, text.length);
    });
  }, [
    composerRef,
    exitComposerHistory,
    markSuggestedUserMessageConsumed,
    setComposerText,
    showSuggestedUserMessage,
    suggestedUserMessage?.text,
  ]);

  // Handle pasted files (clipboard items).
  const handlePaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    exitComposerHistory();
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
    exitComposerHistory();
    const val = e.target.value;
    if (showSuggestedUserMessage && val.trim()) {
      markSuggestedUserMessageConsumed();
    }
    setComposerText(val);
    setComposerGroupMentionTokens((tokens) =>
      pruneComposerGroupMentionTokens({ text: val, tokens }),
    );
    setComposerAgentMentionTokens((tokens) =>
      pruneComposerAgentMentionTokens({ text: val, tokens }),
    );
    const slashModeActive =
      val === val.trimStart() && val.startsWith("/") && !val.slice(1).includes(" ");
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
            setSlashVisibleCount((count) =>
              Math.min(count + SLASH_COMMAND_PAGE_SIZE, slashSuggestions.length),
            );
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
      showSuggestedUserMessage &&
      e.key === "Tab" &&
      !e.shiftKey &&
      !e.ctrlKey &&
      !e.metaKey &&
      !e.altKey
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

    const nativeKeyboardEvent = e.nativeEvent as KeyboardEvent & { keyCode?: number };
    const isComposing = nativeKeyboardEvent.isComposing || nativeKeyboardEvent.keyCode === 229;
    const hasModifier = e.shiftKey || e.ctrlKey || e.metaKey || e.altKey;
    let historySession = composerHistoryRef.current;
    if (historySession && historySession.groupId !== selectedGroupId) {
      exitComposerHistory();
      historySession = null;
    }

    if (historySession) {
      if (e.key === "Escape") {
        e.preventDefault();
        exitComposerHistory();
        return;
      }
      if ((e.key === "ArrowUp" || e.key === "ArrowDown") && !isComposing && !hasModifier) {
        e.preventDefault();
        const move = moveComposerHistory(historySession, e.key === "ArrowUp" ? "older" : "newer");
        composerHistoryRef.current = move.session;
        applyComposerHistoryText(move.text);
        return;
      }
      if (!(["Shift", "Control", "Alt", "Meta"] as string[]).includes(e.key)) {
        exitComposerHistory();
      }
    }

    if (
      e.key === "ArrowUp" &&
      canStartComposerHistory({
        composerText,
        composerGroupSettled,
        selectedGroupId,
        busy,
        menuOpen: showSlashMenu || showMentionMenu,
        isComposing,
        hasModifier,
      })
    ) {
      const entries = buildComposerHistoryEntries(suggestionSourceMessages || recentMessages);
      const session = startComposerHistory(entries, selectedGroupId, composerText);
      if (session) {
        e.preventDefault();
        composerHistoryRef.current = session;
        applyComposerHistoryText(getComposerHistoryText(session));
        return;
      }
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
      const mentionScope =
        lastAt >= 0
          ? resolveControlledComposerMentionContext({
              text: composerText,
              atIndex: lastAt,
              tokens: composerGroupMentionTokens,
            }).scope
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
          setComposerAgentMentionTokens((tokens) =>
            pruneComposerAgentMentionTokens({ text: before, tokens }).concat([token]),
          );
        }
      }
      // @ mentions are text references/autocomplete only. Delivery routing is
      // controlled by explicit recipient chips / the serialized `to` field.
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
        setComposerGroupMentionTokens((tokens) =>
          pruneComposerGroupMentionTokens({ text: before, tokens }).concat([token]),
        );
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

  const isCrossGroup = !!destGroupId && destGroupId !== selectedGroupId;
  const allModeOptions: Array<{ key: ComposerMessageMode; label: string; description: string }> = [
    { key: "send", label: t("modeSend"), description: t("modeSendDesc") },
    { key: "request_reply", label: t("modeSendReply"), description: t("modeSendReplyDesc") },
    { key: "mail", label: t("modeMail"), description: t("modeMailDesc") },
  ];
  const modeOptions = replyTarget
    ? allModeOptions.filter((option) => option.key !== "request_reply")
    : allModeOptions;

  const effectiveMessageMode: ComposerMessageMode = replyTarget
    ? normalizeReplyMessageMode(messageMode)
    : messageMode;
  const activeMode = modeOptions.find((opt) => opt.key === effectiveMessageMode) || modeOptions[0];
  const requestReplyRecipientsReady = hasConcreteReplyRecipients(
    toTokens,
    selectedRemoteGroupIds.length > 0,
  );
  const canSend = getComposerCanSend({
    composerText,
    composerFilesCount: composerFiles.length,
    recipientResolutionBusy: selectedGroupActorsHydrating || recipientActorsBusy,
    messageMode: effectiveMessageMode,
    toTokens,
    hasRemoteGroupSelection: selectedRemoteGroupIds.length > 0,
  });

  const recentChatExcerpt = useMemo(
    () => buildRecentChatExcerptForVoicePrompt(recentMessages),
    [recentMessages],
  );

  const composerAssistantContext = useMemo<Record<string, unknown>>(
    () => ({
      recipients: toTokens,
      message_mode: effectiveMessageMode,
      reply_target: replyTarget
        ? `${replyTarget.by || "unknown"}: ${String(replyTarget.text || "").slice(0, 240)}`
        : "",
      quoted_reference: quotedPresentationRef
        ? getPresentationRefChipLabel(quotedPresentationRef)
        : quotedVoiceDocumentRef
          ? `Voice document: ${getVoiceDocumentRefLabel(quotedVoiceDocumentRef)} (${quotedVoiceDocumentRef.document_path})`
          : "",
      recent_chat_excerpt: recentChatExcerpt,
    }),
    [
      effectiveMessageMode,
      quotedPresentationRef,
      quotedVoiceDocumentRef,
      recentChatExcerpt,
      replyTarget,
      toTokens,
    ],
  );

  const fillPromptDraftFromSpeech = useCallback(
    (draft: string, opts?: { mode?: "replace" | "append" }) => {
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
    },
    [composerRef, setComposerText],
  );

  const fileDisabledReason = (() => {
    if (!selectedGroupId) return t("selectGroupFirst");
    if (busy === "send") return t("busy");
    if (isCrossGroup) return t("crossGroupAttachment");
    return t("attachFile");
  })();
  const sendShortcutLabel = useMemo(() => {
    if (
      typeof navigator !== "undefined" &&
      /Mac|iPhone|iPad|iPod/i.test(navigator.platform || "")
    ) {
      return "⌘+Enter";
    }
    return "Ctrl+Enter";
  }, []);
  const sendButtonTitle =
    effectiveMessageMode === "request_reply" && !requestReplyRecipientsReady
      ? t("modeSendReplyRequiresConcrete")
      : t("sendMessageWithShortcut", {
          shortcut: sendShortcutLabel,
          defaultValue: "Send message ({{shortcut}})",
        });
  const composerPlaceholder = showSuggestedUserMessage
    ? ""
    : isSmallScreen
      ? t("messagePlaceholder")
      : t("messagePlaceholderDesktop");

  return (
    <footer
      className={classNames(
        "relative z-40 flex-shrink-0 border-t px-2 pt-1.5 pb-[calc(0.375rem+env(safe-area-inset-bottom,0px)*0.6)] transition-colors sm:px-2.5 sm:pt-2 sm:pb-[calc(0.5rem+env(safe-area-inset-bottom,0px)*0.6)]",
        // The panel background is 90%+ opaque: a backdrop blur under it is
        // invisible but still re-filters the whole footer on every frame.
        "border-[var(--glass-border)] bg-[var(--glass-panel-bg)]",
      )}
    >
      {/* Reply indicator */}
      {replyTarget && (
        <div
          className={classNames(
            "mb-2.5 flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[11px]",
            isDark
              ? "border-white/[0.06] bg-white/[0.035] text-[var(--color-text-tertiary)]"
              : "border-black/[0.05] bg-black/[0.025] text-gray-500",
          )}
        >
          <ReplyIcon size={12} className="flex-shrink-0 opacity-45" />
          <span className="min-w-0 flex-1 truncate">
            <span className="mr-1 opacity-55">{t("replyingTo")}</span>
            <span
              className={classNames("font-medium", isDark ? "text-slate-300/90" : "text-gray-700")}
            >
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
                : "text-gray-400 hover:bg-black/[0.06] hover:text-gray-600",
            )}
            onClick={onCancelReply}
            title={t("cancelReply")}
            aria-label={t("cancelReply")}
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
          <span
            className={classNames(
              "flex-shrink-0 font-medium",
              isDark ? "text-cyan-100/90" : "text-cyan-700",
            )}
          >
            {t("presentationQuotedViewLabel", { defaultValue: "Quoted view" })}
          </span>
          <span
            className="min-w-0 flex-1 truncate opacity-80"
            title={quotedPresentationRef.title || quotedPresentationRefLabel}
          >
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

      {quotedVoiceDocumentRef && (
        <div
          className={classNames(
            "mb-2.5 flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[11px]",
            isDark
              ? "border-violet-400/12 bg-violet-500/6 text-[var(--color-text-tertiary)]"
              : "border-violet-200/70 bg-violet-50/70 text-gray-600",
          )}
        >
          <span
            className={classNames(
              "flex-shrink-0 font-medium",
              isDark ? "text-violet-100/90" : "text-violet-700",
            )}
          >
            {t("voiceSecretaryQuotedDocumentLabel", { defaultValue: "Quoted document" })}
          </span>
          <span
            className="min-w-0 flex-1 truncate opacity-80"
            title={quotedVoiceDocumentRef.document_path}
          >
            {quotedVoiceDocumentRefLabel}
          </span>
          <button
            className={classNames(
              "rounded-full p-1 transition-colors",
              isDark
                ? "text-[var(--color-text-tertiary)] hover:bg-white/[0.08] hover:text-[var(--color-text-primary)]"
                : "text-gray-400 hover:bg-black/[0.06] hover:text-gray-600",
            )}
            onClick={onClearQuotedVoiceDocumentRef}
            title={t("voiceSecretaryRemoveQuotedDocument", {
              defaultValue: "Remove quoted document",
            })}
            aria-label={t("voiceSecretaryRemoveQuotedDocument", {
              defaultValue: "Remove quoted document",
            })}
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
              removeLabel={t("removeAttachment", { name: f.name })}
            />
          ))}
        </div>
      )}

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
        <div className="relative flex min-w-0 flex-1 flex-col">
          <ComposerRecipientsRow
            isDark={isDark}
            isSmallScreen={isSmallScreen}
            selectedGroupId={selectedGroupId}
            busy={busy}
            actors={actors}
            selectedGroupActorsHydrating={selectedGroupActorsHydrating}
            toTokens={toTokens}
            onToggleRecipient={onToggleRecipient}
            remoteGroups={remoteGroups}
            selectedRemoteGroupIds={selectedRemoteGroupIds}
            onToggleRemoteGroup={onToggleRemoteGroup}
            onClearRecipients={onClearRecipients}
          />

          {/* Row 2 — Textarea */}
          <div className="relative min-w-0 flex-1">
            {mentionOverlay ? (
              <div
                className="pointer-events-none absolute inset-0 overflow-hidden whitespace-pre-wrap break-words border-none px-4 py-3 text-transparent"
                style={{
                  minHeight: `${minComposerHeight}px`,
                  maxHeight: `${maxComposerHeight}px`,
                  fontSize: `${composerFontSize}px`,
                  lineHeight: `${composerLineHeight}px`,
                  border: "none",
                }}
                aria-hidden="true"
              >
                <div
                  style={{ transform: `translateY(-${composerScrollTop}px)` }}
                  dangerouslySetInnerHTML={{ __html: mentionOverlay }}
                />
              </div>
            ) : null}
            <textarea
              ref={composerRef as RefObject<HTMLTextAreaElement>}
              className={classNames(
                "relative w-full bg-transparent border-none py-3 resize-none overflow-y-auto scrollbar-hide focus:outline-none focus:ring-0 focus-visible:shadow-none text-[var(--color-text-primary)] placeholder:text-[var(--color-text-muted)]",
                showSuggestedUserMessage ? "pl-11 pr-4" : "px-4",
              )}
              style={{
                minHeight: `${minComposerHeight}px`,
                maxHeight: `${maxComposerHeight}px`,
                fontSize: `${composerFontSize}px`,
                lineHeight: `${composerLineHeight}px`,
                border: "none",
                outline: "none",
                boxShadow: "none",
              }}
              placeholder={composerPlaceholder}
              rows={1}
              value={composerText}
              onPaste={handlePaste}
              onChange={handleChange}
              onKeyDown={handleKeyDown}
              onPointerDown={exitComposerHistory}
              onScroll={(event) => setComposerScrollTop(event.currentTarget.scrollTop)}
              onBlur={() => setTimeout(() => setShowMentionMenu(false), 150)}
              aria-label={t("messageInput")}
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
                  setSlashVisibleCount((count) =>
                    Math.min(count + SLASH_COMMAND_PAGE_SIZE, slashSuggestions.length),
                  );
                }}
              />
            )}
          </div>
          {/* Row 3 — Action bar */}
          <div
            className={classNames(
              "grid grid-cols-[2.75rem_minmax(0,1fr)_2.75rem_2.75rem] items-center gap-2 px-2 pb-2 pt-1 sm:flex sm:justify-between",
            )}
          >
            <div className="contents sm:flex sm:items-center sm:gap-1.5">
              <button
                className={classNames(
                  "glass-btn flex h-11 w-11 items-center justify-center rounded-lg text-[var(--color-text-secondary)] transition-colors disabled:cursor-not-allowed disabled:text-[var(--color-text-tertiary)] disabled:opacity-60 sm:h-9 sm:w-9",
                  busy !== "send" && selectedGroupId && !isCrossGroup
                    ? isDark
                      ? "hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "hover:bg-black/5 hover:text-gray-800"
                    : "",
                )}
                onClick={() => fileInputRef.current?.click()}
                disabled={!selectedGroupId || busy === "send" || isCrossGroup}
                aria-label={t("attachFile")}
                title={fileDisabledReason}
              >
                <AttachmentIcon size={18} />
              </button>

              <div className="min-w-0 sm:min-w-max">
                <LazyVoiceSecretaryComposerControl
                  isDark={isDark}
                  selectedGroupId={selectedGroupId}
                  busy={busy}
                  disabled={!selectedGroupId || busy === "send" || !composerGroupSettled}
                  variant="assistantRow"
                  captureMode={voiceCaptureMode}
                  onCaptureModeChange={setVoiceCaptureMode}
                  composerText={composerText}
                  composerContext={composerAssistantContext}
                  onQuoteDocument={onQuoteVoiceDocumentRef}
                  onPromptDraft={fillPromptDraftFromSpeech}
                />
              </div>
            </div>

            <div className="contents sm:flex sm:items-center sm:gap-1.5">
              <div ref={modeMenuRef} className="relative z-20">
                <button
                  type="button"
                  className={classNames(
                    "inline-flex h-11 w-11 items-center justify-center gap-0.5 rounded-lg px-0 text-[11px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-60 sm:h-9 sm:w-auto sm:gap-1.5 sm:px-2.5",
                    busy === "send" || !selectedGroupId
                      ? isDark
                        ? "text-[var(--color-text-tertiary)]"
                        : "text-gray-400"
                      : effectiveMessageMode === "request_reply"
                        ? isDark
                          ? "bg-violet-500/18 text-violet-200 hover:bg-violet-500/26"
                          : "bg-violet-100 text-violet-700 hover:bg-violet-200"
                        : effectiveMessageMode === "mail"
                          ? isDark
                            ? "bg-sky-500/14 text-sky-200 hover:bg-sky-500/22"
                            : "bg-sky-50 text-sky-700 hover:bg-sky-100"
                          : isDark
                            ? "text-slate-200 hover:bg-white/10"
                            : "text-gray-700 hover:bg-black/5",
                  )}
                  disabled={busy === "send" || !selectedGroupId}
                  onClick={() => setShowModeMenu((v) => !v)}
                  aria-label={t("messageMode", { mode: activeMode.label })}
                  aria-haspopup="menu"
                  aria-expanded={showModeMenu}
                  title={t("messageMode", { mode: activeMode.label })}
                >
                  {effectiveMessageMode === "request_reply" ? (
                    <ReplyIcon size={13} />
                  ) : effectiveMessageMode === "mail" ? (
                    <InboxIcon size={13} />
                  ) : (
                    <SendIcon size={13} />
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
                    aria-label={t("messageTypeOptions")}
                  >
                    {modeOptions.map((opt) => {
                      const active = effectiveMessageMode === opt.key;
                      const unavailable =
                        opt.key === "request_reply" && !requestReplyRecipientsReady;
                      return (
                        <button
                          key={opt.key}
                          type="button"
                          className={classNames(
                            "w-full rounded-xl px-3 py-2.5 text-left flex items-center gap-2.5 transition-colors disabled:cursor-not-allowed disabled:opacity-50",
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
                          disabled={unavailable}
                          aria-disabled={unavailable}
                          title={unavailable ? t("modeSendReplyRequiresConcrete") : opt.description}
                          onClick={() => {
                            if (unavailable) return;
                            setMessageMode(opt.key);
                            setShowModeMenu(false);
                          }}
                        >
                          <span
                            className={classNames(
                              "w-6 h-6 rounded-md flex items-center justify-center flex-shrink-0",
                              opt.key === "request_reply"
                                ? isDark
                                  ? "bg-violet-500/25 text-violet-200"
                                  : "bg-violet-100 text-violet-700"
                                : opt.key === "mail"
                                  ? isDark
                                    ? "bg-sky-500/20 text-sky-200"
                                    : "bg-sky-50 text-sky-700"
                                  : isDark
                                    ? "bg-slate-700 text-slate-200"
                                    : "bg-gray-100 text-gray-700",
                            )}
                          >
                            {opt.key === "request_reply" ? (
                              <ReplyIcon size={13} />
                            ) : opt.key === "mail" ? (
                              <InboxIcon size={13} />
                            ) : (
                              <SendIcon size={13} />
                            )}
                          </span>
                          <span className="min-w-0 flex-1">
                            <span
                              className={classNames(
                                "block text-sm font-semibold",
                                isDark ? "text-slate-100" : "text-gray-900",
                              )}
                            >
                              {opt.label}
                            </span>
                            <span
                              className={classNames(
                                "block text-[11px]",
                                isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-500",
                              )}
                            >
                              {opt.description}
                            </span>
                          </span>
                          {active && (
                            <span
                              className={classNames(
                                "text-xs font-semibold",
                                isDark ? "text-emerald-300" : "text-emerald-600",
                              )}
                            >
                              ✓
                            </span>
                          )}
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>

              <button
                className={classNames(
                  "flex h-11 w-11 items-center justify-center rounded-lg font-semibold transition-[background-color,box-shadow,transform] duration-150 disabled:cursor-not-allowed sm:h-9 sm:w-[5.5rem]",
                  busy === "send" || !canSend
                    ? isDark
                      ? "bg-white/[0.06] text-[var(--color-text-tertiary)]"
                      : "bg-gray-100 text-gray-400"
                    : "bg-[var(--color-accent-primary)] text-[var(--color-text-inverse)] shadow-[var(--glass-accent-shadow)] hover:brightness-110 active:scale-[0.97]",
                )}
                onClick={onSendMessage}
                disabled={busy === "send" || !canSend}
                aria-label={t("sendMessage")}
                title={sendButtonTitle}
              >
                {busy === "send" ? (
                  <div className="w-4 h-4 border-2 border-white/30 border-t-white rounded-full animate-spin" />
                ) : (
                  <>
                    <SendIcon size={16} className="sm:hidden" />
                    <span className="hidden sm:inline">{t("send")}</span>
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
