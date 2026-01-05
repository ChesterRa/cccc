// ChatComposer - 消息输入组件
import type { Dispatch, RefObject, SetStateAction } from "react";
import { Actor, ReplyTarget } from "../../types";
import { classNames } from "../../utils/classNames";

export interface ChatComposerProps {
  isDark: boolean;
  isSmallScreen: boolean;
  selectedGroupId: string;
  actors: Actor[];
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
  onSendMessage,
  showMentionMenu,
  setShowMentionMenu,
  mentionSuggestions,
  mentionSelectedIndex,
  setMentionSelectedIndex,
  setMentionFilter,
  onAppendRecipientToken,
}: ChatComposerProps) {
  // 处理粘贴文件
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

    // 去重
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

  // 处理文本变化
  const handleChange = (e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const val = e.target.value;
    setComposerText(val);
    const target = e.target;
    target.style.height = "auto";
    target.style.height = Math.min(target.scrollHeight, 140) + "px";

    // 检测 @ 提及
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

  // 处理键盘事件
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
      onCancelReply();
    }
  };

  // 选择提及
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

  return (
    <footer
      className={classNames(
        "flex-shrink-0 border-t px-4 py-3 safe-area-inset-bottom",
        isDark ? "border-slate-800 bg-slate-950/80 backdrop-blur" : "border-gray-200 bg-white/80 backdrop-blur"
      )}
    >
      {/* Reply indicator */}
      {replyTarget && (
        <div className={classNames(
          "mb-2 flex items-center gap-2 text-xs rounded-xl px-3 py-2",
          isDark ? "text-slate-400 bg-slate-900/50" : "text-gray-500 bg-gray-100"
        )}>
          <span className={isDark ? "text-slate-500" : "text-gray-400"}>Replying to</span>
          <span className={classNames("font-medium", isDark ? "text-slate-300" : "text-gray-700")}>{replyTarget.by}</span>
          <span className={classNames("truncate flex-1", isDark ? "text-slate-500" : "text-gray-400")}>"{replyTarget.text}"</span>
          <button
            className={classNames(
              "p-1 rounded-full",
              isDark ? "hover:bg-slate-800 text-slate-400 hover:text-slate-200" : "hover:bg-gray-200 text-gray-400 hover:text-gray-600"
            )}
            onClick={onCancelReply}
            title="Cancel reply"
            aria-label="Cancel reply"
          >
            ×
          </button>
        </div>
      )}

      {/* Recipient Selector */}
      <div className="mb-3 flex items-center gap-2">
        <div className={classNames("text-xs font-medium flex-shrink-0", isDark ? "text-slate-500" : "text-gray-400")}>To</div>
        <div className="flex-1 min-w-0 overflow-x-auto scrollbar-hide sm:overflow-visible">
          <div className="flex items-center gap-1.5 flex-nowrap sm:flex-wrap">
            {["@all", "@foreman", "@peers", ...actors.map((a) => String(a.id || ""))].map((tok) => {
              const t = tok.trim();
              if (!t) return null;
              const active = toTokens.includes(t);
              return (
                <button
                  key={t}
                  className={classNames(
                    "flex-shrink-0 whitespace-nowrap text-[11px] px-2.5 py-1 rounded-full border transition-all",
                    active
                      ? "bg-emerald-600 text-white border-emerald-500 shadow-sm"
                      : isDark
                        ? "bg-slate-900 text-slate-400 border-slate-800 hover:border-slate-600 hover:text-slate-200"
                        : "bg-white text-gray-600 border-gray-200 hover:border-gray-300 hover:text-gray-800"
                  )}
                  onClick={() => onToggleRecipient(t)}
                  disabled={!selectedGroupId || busy === "send"}
                  title={active ? "Remove recipient" : "Add recipient"}
                  aria-pressed={active}
                >
                  {t}
                </button>
              );
            })}
          </div>
        </div>
        {toTokens.length > 0 && (
          <button
            className={classNames(
              "flex-shrink-0 text-[10px] px-2 py-1 rounded-full transition-colors",
              isDark ? "text-slate-500 hover:text-slate-300 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
            )}
            onClick={onClearRecipients}
            disabled={busy === "send"}
            title="Clear recipients"
          >
            clear
          </button>
        )}
      </div>

      {/* File list */}
      {composerFiles.length > 0 && (
        <div className={classNames("mb-3 flex flex-wrap gap-2", isDark ? "text-slate-300" : "text-gray-700")}>
          {composerFiles.map((f, idx) => (
            <span
              key={`${f.name}:${idx}`}
              className={classNames(
                "inline-flex items-center gap-2 rounded-lg border px-2.5 py-1.5 text-xs max-w-full shadow-sm",
                isDark ? "border-slate-700 bg-slate-900" : "border-gray-200 bg-white"
              )}
              title={f.name}
            >
              <span className="truncate">{f.name}</span>
              <button
                className={classNames(
                  "flex-shrink-0 p-0.5 rounded-full",
                  isDark ? "text-slate-400 hover:text-white hover:bg-slate-700" : "text-slate-400 hover:text-gray-700 hover:bg-gray-100"
                )}
                onClick={() => onRemoveComposerFile(idx)}
                title="Remove file"
              >
                ×
              </button>
            </span>
          ))}
        </div>
      )}

      {/* Input row */}
      <div className="flex gap-2 relative items-end">
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
        <button
          className={classNames(
            "rounded-full p-2.5 text-lg transition-colors flex-shrink-0 shadow-sm border",
            isDark
              ? "bg-slate-800 border-slate-700 text-slate-400 hover:text-slate-200 hover:bg-slate-700 hover:border-slate-600"
              : "bg-white border-gray-200 text-gray-400 hover:text-gray-600 hover:bg-gray-50 hover:border-gray-300"
          )}
          onClick={() => fileInputRef.current?.click()}
          disabled={!selectedGroupId || busy === "send"}
          title="Attach file"
        >
          📎
        </button>
        <textarea
          ref={composerRef}
          className={classNames(
            "w-full rounded-3xl border px-4 py-3 text-sm resize-none min-h-[48px] max-h-[140px] transition-all focus:ring-2 focus:ring-offset-1",
            isDark
              ? "bg-slate-900 border-slate-700 text-slate-200 placeholder-slate-500 focus:border-blue-500/50 focus:ring-blue-500/20 focus:ring-offset-slate-900"
              : "bg-white border-gray-200 text-gray-900 placeholder-gray-400 focus:border-blue-400 focus:ring-blue-100 focus:ring-offset-white"
          )}
          placeholder={isSmallScreen ? "Message…" : "Message… (@ to mention, Ctrl+Enter to send)"}
          rows={1}
          value={composerText}
          onPaste={handlePaste}
          onChange={handleChange}
          onKeyDown={handleKeyDown}
          onBlur={() => setTimeout(() => setShowMentionMenu(false), 150)}
          aria-label="Message input"
        />

        {/* Mention menu */}
        {showMentionMenu && mentionSuggestions.length > 0 && (
          <div
            className={classNames(
              "absolute bottom-full left-0 mb-2 w-56 max-h-48 overflow-auto rounded-xl border shadow-xl z-20",
              isDark ? "border-slate-700 bg-slate-900" : "border-gray-200 bg-white"
            )}
            role="listbox"
            aria-label="Mention suggestions"
          >
            {mentionSuggestions.slice(0, 8).map((s, idx) => (
              <button
                key={s}
                className={classNames(
                  "w-full text-left px-4 py-2.5 text-sm transition-colors",
                  isDark ? "text-slate-200" : "text-gray-700",
                  idx === mentionSelectedIndex
                    ? isDark ? "bg-slate-800" : "bg-blue-50 text-blue-700"
                    : isDark ? "hover:bg-slate-800" : "hover:bg-gray-50"
                )}
                onMouseDown={(e) => {
                  e.preventDefault();
                  selectMention(s);
                  composerRef.current?.focus();
                }}
                onMouseEnter={() => setMentionSelectedIndex(idx)}
              >
                {s}
              </button>
            ))}
          </div>
        )}

        {/* Send button */}
        <button
          className={classNames(
            "rounded-full px-5 py-2.5 text-sm font-semibold disabled:opacity-50 min-h-[48px] shadow-sm transition-all flex items-center justify-center",
            busy === "send" || !canSend
              ? isDark ? "bg-slate-800 text-slate-500" : "bg-gray-100 text-gray-400"
              : "bg-blue-600 hover:bg-blue-500 text-white shadow-blue-500/20"
          )}
          onClick={onSendMessage}
          disabled={busy === "send" || !canSend}
          aria-label="Send message"
        >
          {busy === "send" ? <span className="w-5 h-5 border-2 border-white/30 border-t-white rounded-full animate-spin" /> : "Send"}
        </button>
      </div>
    </footer>
  );
}
