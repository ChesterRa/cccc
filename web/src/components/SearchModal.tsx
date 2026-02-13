import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from 'react-i18next';
import { apiJson } from "../services/api";
import { Actor, LedgerEvent } from "../types";
import { formatFullTime, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { useModalA11y } from "../hooks/useModalA11y";

type KindFilter = "all" | "chat" | "notify";

interface SearchModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  actors: Actor[];
  isDark: boolean;
  onReply: (ev: LedgerEvent) => void;
  onJumpToMessage?: (eventId: string) => void;
}

function formatEventText(ev: LedgerEvent): string {
  if (ev.kind === "chat.message" && ev.data && typeof ev.data === "object") {
    const d = ev.data as Record<string, unknown>;
    return typeof d.text === "string" ? d.text : "";
  }
  if (ev.kind === "system.notify" && ev.data && typeof ev.data === "object") {
    const d = ev.data as Record<string, unknown>;
    const kind = typeof d.kind === "string" ? d.kind : "info";
    const title = typeof d.title === "string" ? d.title : "";
    const message = typeof d.message === "string" ? d.message : "";
    const targetId = typeof d.target_actor_id === "string" ? d.target_actor_id : "";
    const target = targetId ? ` → ${targetId}` : "";
    return `[${kind}]${target}: ${title}${message ? ` - ${message}` : ""}`;
  }
  return String(ev.kind || "event");
}

function highlightText(text: string, query: string, isDark: boolean): ReactNode {
  const q = (query || "").trim();
  if (!q) return text;

  const lowerText = text.toLowerCase();
  const lowerQ = q.toLowerCase();
  if (!lowerQ) return text;

  const out: ReactNode[] = [];
  let from = 0;
  let k = 0;
  while (true) {
    const idx = lowerText.indexOf(lowerQ, from);
    if (idx === -1) break;
    if (idx > from) out.push(text.slice(from, idx));
    const matched = text.slice(idx, idx + q.length);
    out.push(
      <mark
        key={`m${k++}-${idx}`}
        className={classNames(
          "px-0.5 rounded",
          isDark ? "bg-amber-500/20 text-amber-200" : "bg-amber-200 text-amber-900"
        )}
      >
        {matched}
      </mark>
    );
    from = idx + q.length;
    if (from >= text.length) break;
  }
  if (from < text.length) out.push(text.slice(from));
  return out;
}

async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // ignore
  }
  try {
    window.prompt("Copy to clipboard:", text);
    return true;
  } catch {
    return false;
  }
}

export function SearchModal({ isOpen, onClose, groupId, actors, isDark, onReply, onJumpToMessage }: SearchModalProps) {
  const { modalRef } = useModalA11y(isOpen, onClose);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<KindFilter>("all");
  const [by, setBy] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const [results, setResults] = useState<LedgerEvent[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const { t } = useTranslation('chat');

  const actorIds = useMemo(() => {
    const ids = actors.map((a) => String(a.id || "")).filter(Boolean);
    ids.sort();
    return ids;
  }, [actors]);

  // Helper to get display name for actor
  const getDisplayName = useMemo(() => {
    const map = new Map<string, string>();
    for (const actor of actors) {
      const id = String(actor.id || "");
      if (id) map.set(id, actor.title || id);
    }
    return (id: string) => {
      if (!id || id === "user") return id;
      return map.get(id) || id;
    };
  }, [actors]);

  useEffect(() => {
    if (!isOpen) return;
    setError("");
    window.setTimeout(() => inputRef.current?.focus(), 0);
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    // When switching groups while open, reset results.
    setResults([]);
    setHasMore(false);
    setError("");
  }, [groupId, isOpen]);

  const doSearch = async (opts?: { before?: string; mode?: "replace" | "prepend" }) => {
    if (!isOpen) return;
    if (!groupId) return;
    setBusy(true);
    setError("");
    try {
      const params = new URLSearchParams();
      params.set("q", query);
      params.set("kind", kind);
      if (by) params.set("by", by);
      params.set("limit", "50");
      if (opts?.before) params.set("before", opts.before);

      const resp = await apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`
      );

      if (!resp.ok) {
        setError(resp.error?.message || "Search failed");
        return;
      }

      const evs = resp.result.events || [];
      setHasMore(!!resp.result.has_more);
      setResults((prev) => {
        if (opts?.mode === "prepend") return evs.concat(prev);
        return evs;
      });
    } finally {
      setBusy(false);
    }
  };

  const loadOlder = async () => {
    const firstId = results[0]?.id ? String(results[0].id) : "";
    if (!firstId) return;
    await doSearch({ before: firstId, mode: "prepend" });
  };

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-stretch sm:items-center justify-center p-0 sm:p-4 animate-fade-in">
      {/* Backdrop */}
      <div
        className={isDark ? "absolute inset-0 bg-black/60" : "absolute inset-0 bg-black/40"}
        onPointerDown={(e) => {
          if (e.target === e.currentTarget) onClose();
        }}
        aria-hidden="true"
      />

      {/* Modal */}
      <div
        className={classNames(
          "relative w-full h-full sm:h-auto sm:max-h-[80vh] sm:max-w-3xl flex flex-col border shadow-2xl animate-scale-in",
          "rounded-none sm:rounded-xl",
          isDark ? "bg-slate-900 border-slate-700" : "bg-white border-gray-200"
        )}
        ref={modalRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="search-modal-title"
      >
        {/* Header */}
        <div className={classNames("flex items-center justify-between px-4 pt-4 pb-3 border-b safe-area-inset-top", isDark ? "border-slate-800" : "border-gray-200")}>
          <div className="min-w-0">
            <h2 id="search-modal-title" className={classNames("text-lg font-semibold truncate", isDark ? "text-slate-100" : "text-gray-900")}>
              {"🔍 "}{t('searchMessages')}
            </h2>
            <div className={classNames("text-xs mt-0.5 truncate", isDark ? "text-slate-400" : "text-gray-500")}>
              {groupId}
            </div>
          </div>
          <button
            onClick={onClose}
            className={classNames(
              "text-xl leading-none min-w-[44px] min-h-[44px] flex items-center justify-center rounded-lg transition-colors",
              isDark ? "text-slate-400 hover:text-slate-200 hover:bg-slate-800" : "text-gray-400 hover:text-gray-600 hover:bg-gray-100"
            )}
            aria-label={t('closeSearchModal')}
          >
            ×
          </button>
        </div>

        {/* Controls */}
        <div className="px-4 py-3 border-b space-y-3 sm:space-y-0 sm:flex sm:items-end sm:gap-3">
          <div className="flex-1 min-w-0">
            <label className={classNames("block text-xs font-medium mb-1", isDark ? "text-slate-300" : "text-gray-700")}>
              {t('query')}
            </label>
            <div className="flex gap-2">
              <input
                ref={inputRef}
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void doSearch({ mode: "replace" });
                }}
                className={classNames(
                  "flex-1 px-3 py-2 border rounded-lg text-sm min-h-[44px]",
                  isDark ? "bg-slate-800 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
                )}
                placeholder={t('searchPlaceholder')}
              />
              <button
                onClick={() => void doSearch({ mode: "replace" })}
                disabled={busy}
                className={classNames(
                  "px-4 py-2 rounded-lg text-sm font-medium min-h-[44px] disabled:opacity-50",
                  isDark ? "bg-emerald-600 hover:bg-emerald-500 text-white" : "bg-emerald-600 hover:bg-emerald-500 text-white"
                )}
              >
                {busy ? "…" : t('common:search')}
              </button>
            </div>
          </div>

          <div className="flex flex-wrap gap-3 items-end">
            <div className="min-w-0">
              <label className={classNames("block text-xs font-medium mb-1", isDark ? "text-slate-300" : "text-gray-700")}>
                {t('kind')}
              </label>
              <div className={classNames("flex items-center gap-1 p-1 rounded-lg", isDark ? "bg-slate-800/60" : "bg-gray-100")}>
                {([
                  ["all", t('kindAll')],
                  ["chat", t('kindChat')],
                  ["notify", t('kindNotify')],
                ] as Array<[KindFilter, string]>).map(([id, label]) => (
                  <button
                    key={id}
                    onClick={() => setKind(id)}
                    className={classNames(
                      "px-2.5 py-1.5 rounded-md text-xs font-medium min-h-[36px] transition-colors",
                      kind === id
                        ? isDark
                          ? "bg-slate-700 text-white"
                          : "bg-white text-gray-900 shadow-sm"
                        : isDark
                          ? "text-slate-300 hover:text-white"
                          : "text-gray-600 hover:text-gray-900"
                    )}
                    aria-pressed={kind === id}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>

            <div className="min-w-0 flex-1 sm:flex-none">
              <label className={classNames("block text-xs font-medium mb-1", isDark ? "text-slate-300" : "text-gray-700")}>
                {t('by')}
              </label>
              <select
                value={by}
                onChange={(e) => setBy(e.target.value)}
                className={classNames(
                  "w-full sm:w-auto px-3 py-2 border rounded-lg text-sm min-h-[44px] sm:min-w-[140px]",
                  isDark ? "bg-slate-800 border-slate-700 text-slate-100" : "bg-white border-gray-300 text-gray-900"
                )}
              >
                <option value="">{t('any')}</option>
                <option value="user">user</option>
                <option value="system">system</option>
                {actorIds.map((id) => (
                  <option key={id} value={id}>
                    {id}
                  </option>
                ))}
              </select>
            </div>
          </div>
        </div>

        {/* Error */}
        {error && (
          <div className={classNames("px-4 py-2 text-sm border-b", isDark ? "border-rose-500/30 bg-rose-500/10 text-rose-300" : "border-rose-300 bg-rose-50 text-rose-700")} role="alert">
            {error}
          </div>
        )}

        {/* Results */}
        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {hasMore && results.length > 0 && (
            <button
              className={classNames(
                "w-full px-4 py-2 rounded-lg text-sm font-medium min-h-[44px] transition-colors",
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-gray-100 hover:bg-gray-200 text-gray-700"
              )}
              onClick={() => void loadOlder()}
              disabled={busy}
            >
              {t('loadOlderResults')}
            </button>
          )}

          {results.map((ev, idx) => {
            const text = formatEventText(ev);
            const evId = ev.id ? String(ev.id) : "";
            const isChat = ev.kind === "chat.message";
            return (
              <div
                key={evId || `r${idx}`}
                className={classNames(
                  "rounded-lg border px-4 py-3",
                  isDark ? "border-slate-700/50 bg-slate-800/40" : "border-gray-200 bg-gray-50"
                )}
              >
                <div className="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className={classNames("text-xs", isDark ? "text-slate-400" : "text-gray-500")} title={formatFullTime(ev.ts)}>
                        {formatTime(ev.ts)}
                      </span>
                      <span className={classNames("text-xs font-medium", isDark ? "text-slate-200" : "text-gray-800")}>
                        {getDisplayName(ev.by || "") || "—"}
                      </span>
                      <span
                        className={classNames(
                          "text-[10px] px-2 py-0.5 rounded-full font-medium",
                          ev.kind === "system.notify"
                            ? isDark
                              ? "bg-blue-500/20 text-blue-300"
                              : "bg-blue-100 text-blue-700"
                            : isDark
                              ? "bg-slate-700/60 text-slate-300"
                              : "bg-white text-gray-600 border border-gray-200"
                        )}
                      >
                        {ev.kind || "event"}
                      </span>
                      {evId && (
                        <span className={classNames("text-[10px] truncate", isDark ? "text-slate-500" : "text-gray-400")} title={evId}>
                          {evId}
                        </span>
                      )}
                    </div>
                    <div className={classNames("mt-2 text-sm whitespace-pre-wrap break-words", isDark ? "text-slate-100" : "text-gray-800")}>
                      {highlightText(text, query, isDark)}
                    </div>
                  </div>

                  <div className="flex items-center gap-2 justify-end sm:flex-col sm:items-end">
                    {isChat && (
                      <button
                        className={classNames(
                          "text-[10px] px-2 py-1 rounded border min-h-[36px] transition-colors",
                          isDark
                            ? "bg-slate-900 border-slate-800 hover:bg-slate-800/60 text-slate-300 hover:text-slate-100"
                            : "bg-white border-gray-200 hover:bg-gray-100 text-gray-600 hover:text-gray-900"
                        )}
                        onClick={() => onReply(ev)}
                        aria-label={`Reply to ${getDisplayName(ev.by || "") || "message"}`}
                        title={t('reply')}
                      >
                        {t('replyTo')}
                      </button>
                    )}
                    {isChat && evId && onJumpToMessage ? (
                      <button
                        className={classNames(
                          "text-[10px] px-2 py-1 rounded border min-h-[36px] transition-colors",
                          isDark
                            ? "bg-slate-900 border-slate-800 hover:bg-slate-800/60 text-slate-300 hover:text-slate-100"
                            : "bg-white border-gray-200 hover:bg-gray-100 text-gray-600 hover:text-gray-900"
                        )}
                        onClick={() => onJumpToMessage(evId)}
                        aria-label={t('openMessageContext')}
                        title={t('openMessage').replace('↗ ', '')}
                      >
                        {t('openMessage')}
                      </button>
                    ) : null}
                    {evId && (
                      <button
                        className={classNames(
                          "text-[10px] px-2 py-1 rounded border min-h-[36px] transition-colors",
                          isDark
                            ? "bg-slate-900 border-slate-800 hover:bg-slate-800/60 text-slate-300 hover:text-slate-100"
                            : "bg-white border-gray-200 hover:bg-gray-100 text-gray-600 hover:text-gray-900"
                        )}
                        onClick={() => void copyToClipboard(evId)}
                        aria-label={t('copyEventId')}
                        title={t('copyEventId')}
                      >
                        {t('copyId')}
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}

          {!busy && results.length === 0 && (
            <div className="text-center py-10">
              <div className="text-3xl mb-2">🔎</div>
              <div className={classNames("text-sm", isDark ? "text-slate-300" : "text-gray-700")}>{t('noResults')}</div>
              <div className={classNames("text-xs mt-1", isDark ? "text-slate-500" : "text-gray-500")}>
                {t('noResultsHint')}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
