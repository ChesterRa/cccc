// ChatComposer renders the chat message composer.
import type { Dispatch, RefObject, SetStateAction } from "react";
import { useEffect, useMemo, useRef, useState } from "react";
import { Actor, GroupMeta, ReplyTarget } from "../../types";
import { classNames } from "../../utils/classNames";
import { AttachmentIcon, SendIcon, ChevronDownIcon, ReplyIcon, CloseIcon, AlertIcon } from "../../components/Icons";
import { ScrollFade } from "../../components/ScrollFade";
import { useTranslation } from 'react-i18next';

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

  // Reply
  replyTarget: ReplyTarget;
  onCancelReply: () => void;

  // Recipients
  toTokens: string[];
  onToggleRecipient: (token: string) => void;
  onClearRecipients: () => void;

  // Files
  composerFiles: File[];
  onRemoveComposerFile: (index: number) => void;
  appendComposerFiles: (files: File[]) => void;
  fileInputRef: RefObject<HTMLInputElement>;

  // Text input
  composerRef: RefObject<HTMLTextAreaElement>;
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
  const modeMenuRef = useRef<HTMLDivElement | null>(null);
  const { t } = useTranslation('chat');

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
        el.style.height = "auto";
        const nextHeight = Math.min(el.scrollHeight, 140);
        el.style.height = `${nextHeight}px`;
        composerHeightRef.current = nextHeight;
      });
    });

    return () => cancelAnimationFrame(rafId);
  }, [composerText, composerRef]);

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

  const chipBaseClass =
    "flex-shrink-0 whitespace-nowrap text-[10px] sm:text-[11px] px-2.5 sm:px-3 rounded-full border transition-all flex items-center justify-center font-medium";

  // Get display name for reply target
  const replyByDisplayName = useMemo(() => {
    if (!replyTarget?.by) return "";
    if (replyTarget.by === "user") return "user";
    const actor = actors.find(a => a.id === replyTarget.by);
    return actor?.title || replyTarget.by;
  }, [replyTarget, actors]);

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
      target.style.height = "auto";
      const nextHeight = Math.min(target.scrollHeight, 140);
      target.style.height = `${nextHeight}px`;
      composerHeightRef.current = nextHeight;
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
    !!selectedGroupId && busy !== "send" && !replyTarget && composerFiles.length === 0;

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

  const fileDisabledReason = (() => {
    if (!selectedGroupId) return t('selectGroupFirst');
    if (busy === "send") return t('busy');
    if (isCrossGroup) return t('crossGroupAttachment');
    return t('attachFile');
  })();

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
      const gid = String(g.group_id || "").trim();
      const title = String(g.title || "").trim();
      const topic = String(g.topic || "").trim();
      const label = title || topic || t('untitledGroup');
      return { gid, label };
    });
  }, [groups, selectedGroupId, t]);

  const groupSelectClass = useMemo(() => {
    if (!canChooseDestGroup || groupOptions.length === 0) {
      return isDark
        ? "bg-slate-900 text-slate-600 border-slate-800"
        : "bg-white text-gray-400 border-gray-200";
    }
    if (isCrossGroup) {
      return isDark
        ? "bg-blue-600/20 text-blue-100 border-blue-500/40 hover:border-blue-400/60"
        : "bg-blue-50 text-blue-700 border-blue-200 hover:border-blue-300";
    }
    // Match the neutral look of recipient buttons
    return isDark
      ? "bg-white/5 text-slate-200 border-white/5 hover:border-white/10"
      : "bg-black/5 text-gray-800 border-transparent hover:border-black/5";
  }, [canChooseDestGroup, groupOptions.length, isCrossGroup, isDark]);

  const groupCaretClass = useMemo(() => {
    if (!canChooseDestGroup || groupOptions.length === 0) return isDark ? "text-slate-600" : "text-gray-400";
    if (isCrossGroup) return isDark ? "text-blue-200" : "text-blue-700";
    return isDark ? "text-slate-400" : "text-gray-500";
  }, [canChooseDestGroup, groupOptions.length, isCrossGroup, isDark]);

  return (
    <footer
      className={classNames(
        "flex-shrink-0 border-t px-3 sm:px-4 py-2.5 sm:py-3 safe-area-bottom-compact transition-colors",
        isDark ? "border-white/5 bg-slate-950/90 backdrop-blur-md" : "border-black/5 bg-white/95 backdrop-blur-md"
      )}
    >
      {/* Reply indicator */}
      {replyTarget && (
        <div className={classNames(
          "mb-3 flex items-center gap-2 text-xs rounded-xl px-3 py-2",
          isDark ? "text-slate-400 bg-white/5" : "text-gray-500 bg-black/5"
        )}>
          <ReplyIcon size={14} className="flex-shrink-0 opacity-60" />
          <span className="font-medium truncate flex-1">
            <span className="opacity-60 mr-1">{t('replyingTo')}</span>
            <span className={isDark ? "text-slate-300" : "text-gray-700"}>{replyByDisplayName}</span>
            <span className="mx-1 opacity-40">"</span>
            <span className="italic opacity-80">{replyTarget.text}</span>
            <span className="opacity-40">"</span>
          </span>
          <button
            className={classNames(
              "p-2.5 -m-1.5 rounded-full transition-colors",
              isDark ? "hover:bg-white/10 text-slate-400 hover:text-white" : "hover:bg-black/10 text-gray-400 hover:text-gray-600"
            )}
            onClick={onCancelReply}
            title={t('cancelReply')}
            aria-label={t('cancelReply')}
          >
            <CloseIcon size={14} />
          </button>
        </div>
      )}

      {/* Recipient Selector Row */}
      <div className="mb-3 sm:mb-4 flex flex-col sm:flex-row sm:items-center gap-2">
        <ScrollFade className="-mx-4 sm:mx-0" innerClassName="flex items-center gap-1.5 sm:gap-2 px-4 sm:px-0" fadeWidth={20}>
          <div className={classNames("text-[10px] font-medium uppercase tracking-wide flex-shrink-0 opacity-50", isDark ? "text-slate-400" : "text-gray-500")}>{t('to', 'To')}</div>

          {/* Group Selector - Styled to match buttons */}
          <div className="relative flex-shrink-0">
            <select
              value={destGroupId || selectedGroupId || ""}
              onChange={(e) => setDestGroupId(e.target.value)}
              style={{ colorScheme: isDark ? "dark" : "light" }}
              className={classNames(
                "appearance-none pr-8 truncate min-w-[120px] max-w-[180px] sm:max-w-[240px]",
                "h-8 transition-colors cursor-pointer", // Fixed height to match buttons
                chipBaseClass,
                groupSelectClass
              )}
              disabled={!canChooseDestGroup || groupOptions.length === 0}
              aria-label={t('destinationGroup')}
            >
              {groupOptions.map((g) => (
                <option key={g.gid} value={g.gid}>
                  {g.label}
                </option>
              ))}
            </select>
            <div className={classNames("pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 opacity-60", groupCaretClass)}>
              <ChevronDownIcon size={12} />
            </div>
          </div>

          <div className="w-[1px] h-4 bg-current opacity-10 flex-shrink-0 mx-1 hidden sm:block" />

          {/* Recipients List - Scrollable horizontally on mobile */}
          <div className={classNames(
            "flex items-center gap-1 sm:gap-1.5 min-w-0 transition-opacity",
            recipientActorsBusy ? "opacity-50 pointer-events-none" : ""
          )}>
            {/* Special tokens */}
            {["@all", "@foreman", "@peers"].map((tok) => {
              const active = toTokens.includes(tok);
              return (
                <button
                  key={tok}
                  className={classNames(
                    "h-7 sm:h-8", // Fixed height, smaller on mobile
                    chipBaseClass,
                    active
                      ? "bg-blue-600 text-white border-blue-500 shadow-sm shadow-blue-500/20"
                      : isDark
                        ? "bg-white/5 text-slate-400 border-white/5 hover:border-white/20 hover:text-slate-200"
                        : "bg-black/5 text-gray-600 border-transparent hover:border-black/10 hover:text-gray-800"
                  )}
                  onClick={() => onToggleRecipient(tok)}
                  disabled={!selectedGroupId || busy === "send"}
                  aria-pressed={active}
                >
                  {tok}
                </button>
              );
            })}
            {/* Actor tokens */}
            {recipientActors.map((actor) => {
              const id = String(actor.id || "");
              if (!id) return null;
              const active = toTokens.includes(id);
              return (
                <button
                  key={id}
                  className={classNames(
                    "h-7 sm:h-8", // Fixed height, smaller on mobile
                    chipBaseClass,
                    active
                      ? "bg-blue-600 text-white border-blue-500 shadow-sm shadow-blue-500/20"
                      : isDark
                        ? "bg-white/5 text-slate-400 border-white/5 hover:border-white/20 hover:text-slate-200"
                        : "bg-black/5 text-gray-600 border-transparent hover:border-black/10 hover:text-gray-800"
                  )}
                  onClick={() => onToggleRecipient(id)}
                  disabled={!selectedGroupId || busy === "send" || !!recipientActorsBusy}
                  aria-pressed={active}
                >
                  {actor.title || id}
                </button>
              );
            })}

            {toTokens.length > 0 && (
              <button
                className={classNames(
                  "p-2 rounded-full transition-all flex-shrink-0 opacity-40 hover:opacity-100",
                  isDark ? "hover:bg-white/10" : "hover:bg-black/10"
                )}
                onClick={onClearRecipients}
                disabled={busy === "send"}
                title={t('clearRecipients')}
              >
                <CloseIcon size={14} />
              </button>
            )}
          </div>
        </ScrollFade>
      </div>

      {/* File list */}
      {composerFiles.length > 0 && (
        <div className="mb-4 flex flex-wrap gap-2 animate-in fade-in slide-in-from-bottom-2 duration-300">
          {composerFiles.map((f, idx) => (
            <div
              key={`${f.name}:${idx}`}
              className={classNames(
                "inline-flex items-center gap-2 rounded-xl border px-3 py-1.5 text-xs max-w-full shadow-sm transition-all",
                isDark ? "border-white/10 bg-slate-900/50 text-slate-300" : "border-black/5 bg-gray-50 text-gray-700"
              )}
            >
              <AttachmentIcon size={12} className="opacity-60" />
              <span className="truncate">{f.name}</span>
              <button
                className={classNames(
                  "flex-shrink-0 p-1.5 -mr-1 rounded-full",
                  isDark ? "hover:bg-white/10 text-slate-400 hover:text-white" : "hover:bg-black/10 text-gray-400 hover:text-gray-700"
                )}
                onClick={() => onRemoveComposerFile(idx)}
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
            "mb-2.5 rounded-lg border px-3 py-2 text-[11px] leading-5",
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

      {/* Main Input Area - Perfectly Centered for Better Alignment */}
      <div className="flex gap-2 sm:gap-2.5 relative items-center">
        <input
          ref={fileInputRef}
          type="file"
          multiple
          className="hidden"
          onChange={(e) => {
            const files = Array.from(e.target.files || []);
            if (files.length > 0) appendComposerFiles(files);
            e.target.value = "";
          }}
        />

        {/* Attachment Button */}
        <button
          className={classNames(
            "w-11 h-11 rounded-xl sm:rounded-2xl flex items-center justify-center transition-all flex-shrink-0 border group",
            isDark
              ? "bg-slate-900 border-white/5 text-slate-400 hover:text-white hover:bg-slate-800"
              : "bg-white border-black/5 text-gray-500 hover:text-gray-900 hover:bg-gray-50"
          )}
          onClick={() => fileInputRef.current?.click()}
          disabled={!selectedGroupId || busy === "send" || isCrossGroup}
          title={fileDisabledReason}
        >
          <AttachmentIcon size={18} className="sm:w-5 sm:h-5 group-active:scale-90 transition-transform" />
        </button>

        {/* Text Area Wrapper */}
        <div className="flex-1 relative min-w-0">
          <textarea
            ref={composerRef}
            className={classNames(
              "w-full rounded-xl sm:rounded-2xl border px-3 sm:px-4 pr-11 sm:pr-12 py-2 sm:py-2.5 text-base sm:text-sm resize-none min-h-[40px] sm:min-h-[44px] max-h-[160px] transition",
              "focus:outline-none focus:ring-2 focus:ring-offset-0 flex items-center",
              isDark
                ? "bg-white/5 border-white/5 text-slate-200 placeholder-slate-500 focus:ring-blue-500/40 focus:border-blue-500/50"
                : "bg-black/5 border-transparent text-gray-900 placeholder-gray-400 focus:ring-blue-400/40 focus:border-blue-400/50"
            )}
            placeholder={isSmallScreen ? t('messagePlaceholder') : t('messagePlaceholderDesktop')}
            rows={1}
            value={composerText}
            onPaste={handlePaste}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            onBlur={() => setTimeout(() => setShowMentionMenu(false), 150)}
            aria-label={t('messageInput')}
          />

          {/* Message Type Selector */}
          <div ref={modeMenuRef} className="absolute right-2 top-1/2 -translate-y-1/2 z-20">
            <button
              type="button"
              className={classNames(
                "w-7 h-7 sm:w-8 sm:h-8 rounded-full transition-colors flex items-center justify-center",
                busy === "send" || !selectedGroupId
                  ? isDark
                    ? "text-slate-500"
                    : "text-gray-400"
                  : messageMode === "task"
                    ? isDark
                      ? "bg-violet-500/20 text-violet-200 hover:bg-violet-500/30"
                      : "bg-violet-100 text-violet-700 hover:bg-violet-200"
                    : messageMode === "attention"
                      ? isDark
                        ? "bg-amber-500/20 text-amber-200 hover:bg-amber-500/30"
                        : "bg-amber-100 text-amber-700 hover:bg-amber-200"
                      : isDark
                        ? "bg-slate-800/70 text-slate-100 ring-1 ring-white/15 hover:bg-slate-700/70"
                        : "bg-white text-gray-700 ring-1 ring-gray-300 hover:bg-gray-50"
              )}
              disabled={busy === "send" || !selectedGroupId}
              onClick={() => setShowModeMenu((v) => !v)}
              aria-label={t('messageType')}
              aria-haspopup="menu"
              aria-expanded={showModeMenu}
              title={t('messageMode', { mode: activeMode.label })}
            >
              {messageMode === "task" ? (
                <ReplyIcon size={12} className="opacity-95" />
              ) : messageMode === "attention" ? (
                <AlertIcon size={12} className="opacity-95" />
              ) : (
                <span className="text-[11px] sm:text-xs font-black italic leading-none">N</span>
              )}
            </button>

            {showModeMenu && (
              <div
                className={classNames(
                  "absolute bottom-full right-0 mb-2 z-40 w-56 sm:w-64 rounded-2xl border p-1.5 shadow-2xl backdrop-blur-xl pointer-events-auto",
                  isDark ? "bg-slate-900/95 border-white/10" : "bg-white/95 border-black/10"
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
                            : "hover:bg-black/5"
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
                                : "bg-gray-100 text-gray-700"
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
                        <span className={classNames("block text-[11px]", isDark ? "text-slate-400" : "text-gray-500")}>
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

          {/* Mention menu */}
          {showMentionMenu && mentionSuggestions.length > 0 && (
            <div
              className={classNames(
                "absolute bottom-full left-0 mb-3 w-64 max-h-60 overflow-auto rounded-2xl border shadow-2xl z-30 animate-in fade-in zoom-in-95 duration-200",
                isDark ? "border-white/10 bg-slate-900 backdrop-blur-xl" : "border-black/5 bg-white backdrop-blur-xl"
              )}
              role="listbox"
            >
              {mentionSuggestions.slice(0, 8).map((s, idx) => (
                <button
                  key={s}
                  className={classNames(
                    "w-full text-left px-4 py-3 text-sm transition-colors",
                    isDark ? "text-slate-200 border-b border-white/5" : "text-gray-700 border-b border-black/5",
                    idx === mentionSelectedIndex
                      ? isDark ? "bg-blue-600/30 text-blue-300" : "bg-blue-50 text-blue-700 font-medium"
                      : isDark ? "hover:bg-white/5" : "hover:bg-gray-50"
                  )}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    selectMention(s);
                    composerRef.current?.focus();
                  }}
                  onMouseEnter={() => setMentionSelectedIndex(idx)}
                >
                  <span className="opacity-60 mr-1">@</span>{s}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Send button - Using icon for modern feel */}
        <button
          className={classNames(
            "w-11 h-11 sm:min-w-[6.25rem] sm:px-3 rounded-xl sm:rounded-2xl flex items-center justify-center transition-all flex-shrink-0 disabled:opacity-50",
            busy === "send" || !canSend
              ? isDark ? "bg-slate-800 text-slate-500" : "bg-gray-100 text-gray-400"
              : "bg-blue-600 hover:bg-blue-500 text-white shadow-lg shadow-blue-500/30 group active:scale-95"
          )}
          onClick={onSendMessage}
          disabled={busy === "send" || !canSend}
          aria-label={t('sendMessage')}
          title={t('sendMessage')}
        >
          {busy === "send" ? (
            <div className="w-4 h-4 sm:w-5 sm:h-5 border-2 border-white/30 border-t-white rounded-full animate-spin" />
          ) : (
            <>
              <SendIcon size={18} className="sm:hidden" />
              <span className="hidden sm:inline font-bold">{t('send')}</span>
            </>
          )}
        </button>
      </div>
    </footer>
  );
}
