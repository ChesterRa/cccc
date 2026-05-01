// ChatComposer renders the chat message composer.
import type { Dispatch, RefObject, SetStateAction } from "react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Actor, GroupMeta, LedgerEvent, PresentationMessageRef, ReplyTarget } from "../../types";
import { classNames } from "../../utils/classNames";
import { AttachmentIcon, SendIcon, ChevronDownIcon, ReplyIcon, CloseIcon, AlertIcon, PetIcon } from "../../components/Icons";
import { ScrollFade } from "../../components/ScrollFade";
import { getPresentationRefChipLabel } from "../../utils/presentationRefs";
import { useTranslation } from 'react-i18next';
import { VoiceSecretaryComposerControl, type VoiceSecretaryCaptureMode } from "./VoiceSecretaryComposerControl";
import { GroupCombobox } from "../../components/GroupCombobox";
import { updateSettings } from "../../services/api";
import { useBuiltInAssistantStore, useGroupStore, useUIStore } from "../../stores";

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
  recipientActors: Actor[];
  recipientActorsBusy?: boolean;
  groups: GroupMeta[];
  destGroupId: string;
  setDestGroupId: (groupId: string) => void;
  destGroupScopeLabel?: string;
  busy: string;
  recentMessages?: LedgerEvent[];

  // Reply
  replyTarget: ReplyTarget;
  onCancelReply: () => void;
  quotedPresentationRef: PresentationMessageRef | null;
  onClearQuotedPresentationRef: () => void;

  // Recipients
  toTokens: string[];
  onToggleRecipient: (token: string) => void;
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
  mentionSuggestions: string[];
  mentionSelectedIndex: number;
  setMentionSelectedIndex: Dispatch<SetStateAction<number>>;
  setMentionFilter: Dispatch<SetStateAction<string>>;
  onAppendRecipientToken: (token: string) => void;
}


export function ChatComposer({
  isDark,
  isSmallScreen,
  selectedGroupId,
  actors,
  recipientActors,
  recipientActorsBusy,
  groups,
  destGroupId,
  setDestGroupId,
  destGroupScopeLabel: _destGroupScopeLabel,
  busy,
  recentMessages = [],
  replyTarget,
  onCancelReply,
  quotedPresentationRef,
  onClearQuotedPresentationRef,
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
  onAppendRecipientToken,
}: ChatComposerProps) {
  const composerHeightRef = useRef(0);
  const isUserInputRef = useRef(false);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const [voiceCaptureMode, setVoiceCaptureMode] = useState<VoiceSecretaryCaptureMode>("prompt");
  const modeMenuRef = useRef<HTMLDivElement | null>(null);
  const { t } = useTranslation('chat');
  const groupSettings = useGroupStore((state) => state.groupSettings);
  const refreshSettings = useGroupStore((state) => state.refreshSettings);
  const refreshInternalRuntimeActors = useGroupStore((state) => state.refreshInternalRuntimeActors);
  const requestAssistantOpen = useBuiltInAssistantStore((state) => state.requestOpen);
  const showError = useUIStore((state) => state.showError);
  const showNotice = useUIStore((state) => state.showNotice);
  const [petBusy, setPetBusy] = useState(false);
  const petEnabled = Boolean(groupSettings?.desktop_pet_enabled);

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

  useEffect(() => {
    if (!selectedGroupId || groupSettings) return;
    void refreshSettings(selectedGroupId);
  }, [groupSettings, refreshSettings, selectedGroupId]);

  const activatePet = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid || busy === "send" || petBusy) return;
    if (petEnabled) {
      requestAssistantOpen(gid, "pet");
      return;
    }
    setPetBusy(true);
    try {
      const resp = await updateSettings(gid, { desktop_pet_enabled: true });
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      await refreshSettings(gid);
      await refreshInternalRuntimeActors(gid);
      showNotice({
        message: t("builtInAssistantPetEnabled", { defaultValue: "PET enabled for this group." }),
      });
      requestAssistantOpen(gid, "pet");
    } catch {
      showError(t("builtInAssistantPetToggleFailed", { defaultValue: "Failed to update PET." }));
    } finally {
      setPetBusy(false);
    }
  }, [
    busy,
    petBusy,
    petEnabled,
    refreshInternalRuntimeActors,
    refreshSettings,
    requestAssistantOpen,
    selectedGroupId,
    showError,
    showNotice,
    t,
  ]);

  const chipBaseClass =
    "flex h-6 flex-shrink-0 items-center justify-center whitespace-nowrap rounded-lg border px-2 text-[10px] font-medium leading-none transition-all sm:px-2.5 sm:text-[11px]";
  const chipActiveClass = isDark
    ? "border-white bg-white text-[rgb(20,20,22)] shadow-none"
    : "border-[rgb(35,36,37)] bg-[rgb(35,36,37)] text-white shadow-none";
  const chipInactiveClass = isDark
    ? "bg-white/[0.06] text-[var(--color-text-secondary)] border-white/[0.08] hover:bg-white/[0.1] hover:border-white/[0.14] hover:text-[var(--color-text-primary)]"
    : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)] border-transparent hover:bg-[rgb(237,237,237)] hover:border-black/5 hover:text-[rgb(20,20,22)]";

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
  const recipientLabelMap = useMemo(() => {
    const map = new Map<string, { label: string; secondary?: string }>();
    for (const actor of recipientActors) {
      const id = String(actor.id || "").trim();
      if (!id) continue;
      const title = String(actor.title || "").trim();
      map.set(id, title && title !== id ? { label: title, secondary: id } : { label: title || id });
    }
    return map;
  }, [recipientActors]);
  const renderRecipientChipContent = useCallback((label: string) => (
    <span className="truncate">{label}</span>
  ), []);

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
    isUserInputRef.current = true;
    setComposerText(val);
    const target = e.target;
    // Use requestAnimationFrame to avoid forced reflow during layout.
    requestAnimationFrame(() => {
      resizeComposer(target);
    });

    // Detect @ mentions for the recipient helper menu.
    const lastAt = val.lastIndexOf("@");
    if (lastAt >= 0) {
      const afterAt = val.slice(lastAt + 1);
      if (
        (lastAt === 0 || val[lastAt - 1] === " " || val[lastAt - 1] === "\n") &&
        !afterAt.includes(" ") &&
        !afterAt.includes("\n")
      ) {
        setMentionFilter(afterAt);
        setShowMentionMenu(true);
        setMentionSelectedIndex(0);
      } else {
        setShowMentionMenu(false);
      }
    } else {
      setShowMentionMenu(false);
    }
  };

  // Handle keyboard shortcuts and mention navigation.
  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (showMentionMenu && mentionSuggestions.length > 0) {
      const maxIndex = Math.min(mentionSuggestions.length, 8) - 1;
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
    if (e.key === "Enter" && !showMentionMenu) {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        onSendMessage();
      }
    } else if (e.key === "Escape") {
      setShowMentionMenu(false);
      setShowModeMenu(false);
      onCancelReply();
    }
  };

  // Select a mention from the menu.
  const selectMention = (selected: string | undefined) => {
    if (!selected) return;
    const lastAt = composerText.lastIndexOf("@");
    if (lastAt >= 0) {
      const before = composerText.slice(0, lastAt);
      setComposerText(before + selected + " ");
    }
    if (!toTokens.includes(selected)) {
      onAppendRecipientToken(selected);
    }
    setShowMentionMenu(false);
    setMentionSelectedIndex(0);
  };

  const canSend = composerText.trim() || composerFiles.length > 0;
  const isAttention = priority === "attention";
  const isCrossGroup = !!destGroupId && destGroupId !== selectedGroupId;
  const canChooseDestGroup =
    !!selectedGroupId && busy !== "send" && !replyTarget && !quotedPresentationRef && composerFiles.length === 0;

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

  const groupOptions = useMemo(() => {
    const cur = String(selectedGroupId || "").trim();
    const list = (groups || []).filter((g) => String(g.group_id || "").trim());
    // Prefer showing the current group first.
    const sorted = list.slice().sort((a, b) => {
      const aId = String(a.group_id || "");
      const bId = String(b.group_id || "");
      if (aId === cur && bId !== cur) return -1;
      if (bId === cur && aId !== cur) return 1;
      const aTitle = String(a.title || "").trim().toLowerCase();
      const bTitle = String(b.title || "").trim().toLowerCase();
      if (aTitle && bTitle) return aTitle.localeCompare(bTitle);
      if (aTitle && !bTitle) return -1;
      if (!aTitle && bTitle) return 1;
      return aId.localeCompare(bId);
    });
    return sorted.map((g) => {
      const value = String(g.group_id || "").trim();
      const title = String(g.title || "").trim();
      const topic = String(g.topic || "").trim();
      const label = title || topic || t('untitledGroup');
      return {
        value,
        label,
        description: label !== value ? value : undefined,
        keywords: [value, title, topic].filter(Boolean),
      };
    });
  }, [groups, selectedGroupId, t]);

  const groupSelectClass = useMemo(() => {
    if (!canChooseDestGroup || groupOptions.length === 0) {
      return isDark
        ? "bg-white/[0.07] text-[var(--color-text-tertiary)] border-white/[0.08]"
        : "bg-white text-gray-400 border-gray-200";
    }
    if (isCrossGroup) {
      return chipActiveClass;
    }
    return chipInactiveClass;
  }, [canChooseDestGroup, chipActiveClass, chipInactiveClass, groupOptions.length, isCrossGroup, isDark]);

  return (
    <footer
      className={classNames(
        "relative z-40 flex-shrink-0 border-t px-2 py-1.5 safe-area-bottom-compact transition-colors sm:px-2.5 sm:py-2",
        isDark ? "border-white/5 bg-slate-950/72 backdrop-blur-md" : "border-black/5 bg-white/78 backdrop-blur-md"
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
              <div
                key={`${f.name}:${idx}`}
                className={classNames(
                  "inline-flex max-w-full items-center gap-2 rounded-xl border px-3 py-1.5 text-xs shadow-sm transition-all",
                  "border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] text-[var(--color-text-secondary)]"
                )}
              >
                <AttachmentIcon
                  size={12}
                  className="flex-shrink-0 text-[var(--color-text-tertiary)]"
                />
                <span
                  className="truncate font-medium text-[var(--color-text-primary)]"
                  title={f.name}
                >
                  {f.name}
                </span>
                <button
                  className={classNames(
                    "flex-shrink-0 p-1.5 -mr-1 rounded-full",
                    "text-[var(--color-text-tertiary)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)]"
                  )}
                  onClick={() => onRemoveComposerFile(idx)}
                  aria-label={t('removeAttachment', { name: f.name })}
                  title={t('removeAttachment', { name: f.name })}
                >
                  <CloseIcon size={14} />
                </button>
              </div>
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
            className={classNames(
              "relative flex min-w-0 flex-1 flex-col transition-[background-color] duration-200",
              isDark
                ? "bg-white/[0.025] focus-within:bg-white/[0.045]"
                : "bg-white/55 focus-within:bg-white/80",
            )}
          >
            {/* Row 1 — Recipients */}
            <div
              className={classNames(
                "flex items-center gap-1.5 border-b px-2.5 py-1",
                isDark ? "border-white/[0.04]" : "border-black/[0.04]",
              )}
            >
              <span className={classNames("flex-shrink-0 text-[10px] font-medium tracking-[0.08em]", isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-400")}>
                {t('to', 'To')}
              </span>

              <div className="flex-shrink-0">
                <GroupCombobox
                  items={groupOptions}
                  value={destGroupId || selectedGroupId || ""}
                  onChange={setDestGroupId}
                  placeholder={t('destinationGroup')}
                  searchPlaceholder={t('searchDestinationGroup', { defaultValue: 'Search groups...' })}
                  emptyText={t('noMatchingGroups', { defaultValue: 'No matching groups' })}
                  ariaLabel={t('destinationGroup')}
                  triggerClassName={classNames(
                    "inline-flex w-auto min-w-[68px] max-w-[148px] sm:max-w-[196px]",
                    "h-6 cursor-pointer gap-1 px-2 text-[10px]",
                    chipBaseClass,
                    groupSelectClass,
                  )}
                  contentClassName="max-w-[min(20rem,calc(100vw-1rem))]"
                  descriptionClassName="text-[10px]"
                  caretClassName={classNames(
                    !canChooseDestGroup || groupOptions.length === 0
                      ? (isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-400")
                      : isCrossGroup
                        ? "text-[var(--color-text-primary)]"
                        : (isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-500")
                  )}
                  disabled={!canChooseDestGroup || groupOptions.length === 0}
                />
              </div>

              <ScrollFade
                className="min-w-0 flex-1"
                innerClassName="w-full max-w-full"
                fadeWidth={20}
              >
                <div
                  className={classNames(
                    "flex min-w-max items-center gap-1 transition-opacity",
                    recipientActorsBusy ? "opacity-50 pointer-events-none" : "",
                  )}
                >
                  {["@all", "@foreman", "@peers"].map((tok) => {
                    const active = toTokens.includes(tok);
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
                        disabled={!selectedGroupId || busy === "send"}
                        aria-pressed={active}
                      >
                        {renderRecipientChipContent(tok)}
                      </button>
                    );
                  })}
                  {recipientActors.map((actor) => {
                    const id = String(actor.id || "");
                    if (!id) return null;
                    const active = toTokens.includes(id);
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
                        disabled={!selectedGroupId || busy === "send" || !!recipientActorsBusy}
                        aria-pressed={active}
                      >
                        {renderRecipientChipContent(actor.title || id)}
                      </button>
                    );
                  })}
                </div>
              </ScrollFade>

              {toTokens.length > 0 && (
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
              <textarea
                ref={composerRef as RefObject<HTMLTextAreaElement>}
                className={classNames(
                  "w-full bg-transparent border-0 px-4 py-3 resize-none overflow-y-auto scrollbar-hide focus:outline-none focus:ring-0",
                  isDark
                    ? "text-[var(--color-text-primary)] placeholder:text-[var(--color-text-tertiary)]"
                    : "text-gray-900 placeholder-gray-400",
                )}
                style={{
                  minHeight: `${Math.max(baseComposerHeight + 6, 52)}px`,
                  maxHeight: `${maxComposerHeight}px`,
                  fontSize: `${composerFontSize}px`,
                  lineHeight: `${composerLineHeight}px`,
                }}
                placeholder={isSmallScreen ? t('messagePlaceholder') : t('messagePlaceholderDesktop')}
                rows={1}
                value={composerText}
                onPaste={handlePaste}
                onChange={handleChange}
                onKeyDown={handleKeyDown}
                onBlur={() => setTimeout(() => setShowMentionMenu(false), 150)}
                aria-label={t('messageInput')}
              />

              {/* Mention menu */}
              {showMentionMenu && mentionSuggestions.length > 0 && (
                <div
                  className={classNames(
                    "glass-panel absolute bottom-full left-2 mb-3 w-64 max-h-60 overflow-auto scrollbar-subtle rounded-2xl border shadow-2xl z-30 animate-in fade-in zoom-in-95 duration-200",
                  )}
                  role="listbox"
                >
                  {mentionSuggestions.slice(0, 8).map((s, idx) => (
                    (() => {
                      const option = recipientLabelMap.get(s);
                      const primaryLabel = option?.label || s;
                      const secondaryLabel = option?.secondary;
                      return (
                        <button
                          key={s}
                          className={classNames(
                            "w-full text-left px-4 py-3 text-sm transition-colors",
                            isDark ? "text-slate-200 border-b border-white/5" : "text-gray-700 border-b border-black/5",
                            idx === mentionSelectedIndex
                              ? "bg-[var(--glass-tab-bg-active)] text-[var(--color-text-primary)] font-medium"
                              : isDark ? "hover:bg-white/5" : "hover:bg-gray-50",
                          )}
                          onMouseDown={(e) => {
                            e.preventDefault();
                            selectMention(s);
                            composerRef.current?.focus();
                          }}
                          onMouseEnter={() => setMentionSelectedIndex(idx)}
                        >
                          <div className="flex items-center gap-2 min-w-0">
                            <span className="opacity-60 flex-shrink-0">@</span>
                            <div className="min-w-0">
                              <div className="truncate">{primaryLabel}</div>
                              {secondaryLabel ? (
                                <div className={classNames("truncate text-[11px]", isDark ? "text-slate-400" : "text-gray-500")}>
                                  @{secondaryLabel}
                                </div>
                              ) : null}
                            </div>
                          </div>
                        </button>
                      );
                    })()
                  ))}
                </div>
              )}
            </div>

            {/* Row 3 — Action bar */}
            <div
              className={classNames(
                "flex items-center justify-between gap-2 px-2 pb-2 pt-1",
              )}
            >
              <div className="flex items-center gap-1.5">
                <button
                  className={classNames(
                    "glass-btn flex h-9 w-9 items-center justify-center rounded-lg text-[var(--color-text-secondary)] transition-colors disabled:cursor-not-allowed disabled:text-[var(--color-text-tertiary)] disabled:opacity-60",
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

                <button
                  type="button"
                  className={classNames(
                    "relative flex h-9 w-9 items-center justify-center rounded-lg border text-[var(--color-text-secondary)] transition-colors disabled:cursor-not-allowed disabled:opacity-60",
                    petEnabled
                      ? isDark
                        ? "border-amber-300/25 bg-amber-300/12 text-amber-100 hover:bg-amber-300/18"
                        : "border-amber-200 bg-amber-50 text-amber-800 hover:bg-amber-100"
                      : isDark
                        ? "border-white/[0.08] bg-white/[0.04] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                        : "border-black/[0.06] bg-white hover:bg-black/5 hover:text-gray-800",
                  )}
                  onClick={() => void activatePet()}
                  disabled={!selectedGroupId || busy === "send" || petBusy}
                  aria-label={
                    petEnabled
                      ? t("builtInAssistantPetOpen", { defaultValue: "Open PET" })
                      : t("builtInAssistantPetTurnOn", { defaultValue: "Turn PET on" })
                  }
                  title={
                    petEnabled
                      ? t("builtInAssistantPetOpen", { defaultValue: "Open PET" })
                      : t("builtInAssistantPetTurnOn", { defaultValue: "Turn PET on" })
                  }
                >
                  <PetIcon size={17} aria-hidden="true" />
                  <span
                    className={classNames(
                      "absolute right-1.5 top-1.5 h-1.5 w-1.5 rounded-full",
                      petEnabled
                        ? "bg-emerald-500 shadow-[0_0_0_3px_rgba(16,185,129,0.18)]"
                        : isDark ? "bg-white/20" : "bg-gray-300",
                    )}
                    aria-hidden="true"
                  />
                </button>

                <VoiceSecretaryComposerControl
                  isDark={isDark}
                  selectedGroupId={selectedGroupId}
                  busy={busy}
                  disabled={!selectedGroupId || busy === "send"}
                  variant="assistantRow"
                  captureMode={voiceCaptureMode}
                  onCaptureModeChange={setVoiceCaptureMode}
                  composerText={composerText}
                  composerContext={composerAssistantContext}
                  onPromptDraft={fillPromptDraftFromSpeech}
                />
              </div>

              <div className="flex items-center gap-1.5">
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

                <button
                  className={classNames(
                    "flex h-9 w-9 items-center justify-center rounded-lg font-semibold transition-[background-color,box-shadow,transform] duration-150 disabled:cursor-not-allowed sm:w-[5.5rem]",
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
