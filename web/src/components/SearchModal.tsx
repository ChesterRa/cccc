import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../services/api";
import { Actor, LedgerEvent } from "../types";
import { formatFullTime, formatTime } from "../utils/time";
import { classNames } from "../utils/classNames";
import { useCopyFeedback } from "../hooks/useCopyFeedback";
import { useModalA11y } from "../hooks/useModalA11y";
import { SearchIcon } from "./Icons";
import { Button } from "./ui/button";
import { Input } from "./ui/input";
import { ModalFrame } from "./modals/ModalFrame";
import { getMessageInsight } from "../utils/messagePerspective";

type KindFilter = "all" | "chat" | "notify";
type SearchCriteria = { query: string; kind: KindFilter; by: string };

interface SearchModalProps {
  isOpen: boolean;
  onClose: () => void;
  groupId: string;
  groupTitle?: string;
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

function highlightText(text: string, query: string, _isDark?: boolean): ReactNode {
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
        className="px-0.5 rounded bg-amber-200 text-amber-900 dark:bg-amber-500/20 dark:text-amber-200"
      >
        {matched}
      </mark>,
    );
    from = idx + q.length;
    if (from >= text.length) break;
  }
  if (from < text.length) out.push(text.slice(from));
  return out;
}

export function SearchModal({
  isOpen,
  onClose,
  groupId,
  groupTitle,
  actors,
  isDark,
  onReply,
  onJumpToMessage,
}: SearchModalProps) {
  const copyWithFeedback = useCopyFeedback();
  const inputRef = useRef<HTMLInputElement | null>(null);
  const { modalRef } = useModalA11y(isOpen, onClose, { initialFocusRef: inputRef });
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<KindFilter>("all");
  const [by, setBy] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  const [results, setResults] = useState<LedgerEvent[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const { t } = useTranslation("chat");

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

  const requestId = useRef(0);
  const lastSearch = useRef<SearchCriteria | null>(null);
  const [searchedQuery, setSearchedQuery] = useState<string | null>(null);

  useEffect(() => {
    setResults([]);
    setHasMore(false);
    setError("");
    setBusy(false);
    setSearchedQuery(null);
    lastSearch.current = null;
    setBy("");
    setKind("all");
    return () => {
      // A closed search or another Group must never receive an older response.
      requestId.current += 1;
    };
  }, [groupId, isOpen]);

  const doSearch = async (criteria: SearchCriteria, before?: string) => {
    if (!isOpen || !groupId) return;
    const id = ++requestId.current;
    lastSearch.current = criteria;
    setBusy(true);
    setError("");
    if (!before) {
      setResults([]);
      setHasMore(false);
      setSearchedQuery(criteria.query);
    }
    try {
      const params = new URLSearchParams({ q: criteria.query, kind: criteria.kind, limit: "50" });
      if (criteria.by) params.set("by", criteria.by);
      if (before) params.set("before", before);
      const resp = await apiJson<{ events: LedgerEvent[]; has_more: boolean; count: number }>(
        `/api/v1/groups/${encodeURIComponent(groupId)}/ledger/search?${params.toString()}`,
      );
      if (id !== requestId.current) return;
      if (!resp.ok) {
        setError(resp.error?.message || t("searchFailed"));
        return;
      }
      const events = resp.result.events || [];
      setHasMore(!!resp.result.has_more);
      setResults((previous) => (before ? events.concat(previous) : events));
    } catch {
      if (id === requestId.current) setError(t("searchFailed"));
    } finally {
      if (id === requestId.current) setBusy(false);
    }
  };

  const filterSearch = (next: Partial<Pick<SearchCriteria, "kind" | "by">>) => {
    if (next.kind !== undefined) setKind(next.kind);
    if (next.by !== undefined) setBy(next.by);
    if (lastSearch.current) void doSearch({ ...lastSearch.current, ...next });
  };
  const loadOlder = () => {
    const before = results[0]?.id;
    if (before && lastSearch.current) void doSearch(lastSearch.current, before);
  };

  if (!isOpen) return null;

  const titleContent = (
    <div className="min-w-0">
      <h2 className="text-lg font-semibold truncate text-[var(--color-text-primary)]">
        <span className="inline-flex items-center gap-2">
          <SearchIcon size={18} />
          <span>{t("searchMessages")}</span>
        </span>
      </h2>
      <div className="text-xs mt-0.5 truncate text-[var(--color-text-muted)]">
        {groupTitle || groupId}
      </div>
    </div>
  );

  return (
    <ModalFrame
      isOpen={isOpen}
      isDark={isDark}
      onClose={onClose}
      titleId="search-modal-title"
      title={titleContent}
      closeAriaLabel={t("closeSearchModal")}
      surface="solid"
      panelClassName="w-full h-full sm:h-[min(70dvh,520px)] sm:max-w-3xl"
      modalRef={modalRef}
    >
      <form
        className="shrink-0 space-y-3 border-b border-[var(--glass-border-subtle)] px-4 py-3 sm:px-5"
        onSubmit={(event) => {
          event.preventDefault();
          void doSearch({ query: query.trim(), kind, by });
        }}
      >
        <div className="flex min-w-0 gap-2">
          <label htmlFor="message-search-query" className="sr-only">
            {t("query")}
          </label>
          <Input
            id="message-search-query"
            ref={inputRef}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            className="min-h-11 min-w-0 flex-1"
            placeholder={t("searchPlaceholder")}
          />
          <Button type="submit" className="min-h-11 shrink-0">
            {t("common:search")}
          </Button>
        </div>
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          <div className="flex flex-wrap items-center gap-1" role="group" aria-label={t("kind")}>
            {(
              [
                ["all", t("kindAll")],
                ["chat", t("kindChat")],
                ["notify", t("kindNotify")],
              ] as Array<[KindFilter, string]>
            ).map(([id, label]) => (
              <Button
                key={id}
                type="button"
                variant="ghost"
                size="sm"
                onClick={() => filterSearch({ kind: id })}
                className={classNames(
                  "min-h-9 border",
                  kind === id
                    ? "border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] font-semibold text-[var(--color-text-primary)]"
                    : "border-transparent",
                )}
                aria-pressed={kind === id}
              >
                {label}
              </Button>
            ))}
          </div>
          <div className="flex min-w-0 flex-1 items-center gap-2 sm:justify-end">
            <span className="shrink-0 text-xs text-[var(--color-text-secondary)]">{t("by")}</span>
            <select
              value={by}
              onChange={(event) => filterSearch({ by: event.target.value })}
              aria-label={t("by")}
              className="min-h-9 min-w-0 w-44 max-w-full rounded-lg border border-[var(--color-border-secondary)] bg-[var(--color-bg-primary)] px-2 text-sm text-[var(--color-text-primary)]"
            >
              <option value="">{t("any")}</option>
              <option value="user">{t("searchSenderUser")}</option>
              <option value="system">{t("searchSenderSystem")}</option>
              {actorIds.map((id) => (
                <option key={id} value={id}>
                  {getDisplayName(id)}
                </option>
              ))}
            </select>
          </div>
        </div>
      </form>

      {/* Error */}
      {error && (
        <div
          className="px-4 py-2.5 mx-4 mt-3 text-sm rounded-xl border border-rose-500/20 bg-rose-500/5 text-rose-600 dark:text-rose-400 flex-shrink-0"
          role="alert"
        >
          {error}
        </div>
      )}

      {/* Results */}
      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3 sm:px-5" aria-busy={busy}>
        {searchedQuery !== null && (
          <p role="status" className="mb-2 text-xs text-[var(--color-text-secondary)]">
            {busy
              ? t("searching")
              : searchedQuery
                ? t("searchResultsFor", { query: searchedQuery })
                : t("searchFilteredResults")}
          </p>
        )}
        {hasMore && results.length > 0 && (
          <Button
            type="button"
            variant="secondary"
            className="w-full rounded-lg"
            onClick={() => void loadOlder()}
            disabled={busy}
          >
            {t("loadOlderResults")}
          </Button>
        )}

        {results.map((ev, idx) => {
          const text = formatEventText(ev);
          const insight = ev.kind === "chat.message" ? getMessageInsight(ev.data) : "";
          const evId = ev.id ? String(ev.id) : "";
          const isChat = ev.kind === "chat.message";
          return (
            <article
              key={evId || `r${idx}`}
              className="border-b border-[var(--glass-border-subtle)] py-4 last:border-b-0"
            >
              <div className="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span
                      className="text-xs text-[var(--color-text-muted)]"
                      title={formatFullTime(ev.ts)}
                    >
                      {formatTime(ev.ts)}
                    </span>
                    <span className="text-xs font-medium text-[var(--color-text-primary)]">
                      {getDisplayName(ev.by || "") || "—"}
                    </span>
                    {ev.kind === "system.notify" && (
                      <span className="text-xs text-[var(--color-text-tertiary)]">
                        {t("kindNotify")}
                      </span>
                    )}
                  </div>
                  <div className="mt-2 text-sm whitespace-pre-wrap break-words text-[var(--color-text-primary)]">
                    {highlightText(text, searchedQuery || "", isDark)}
                  </div>
                  {insight ? (
                    <div className="mt-3 border-t border-[var(--glass-border-subtle)] pt-2">
                      <div className="mb-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-[var(--color-text-muted)]">
                        {t("senderPerspective")}
                      </div>
                      <div className="text-sm whitespace-pre-wrap break-words text-[var(--color-text-secondary)]">
                        {highlightText(insight, searchedQuery || "", isDark)}
                      </div>
                    </div>
                  ) : null}
                </div>

                <div className="flex flex-wrap items-center gap-1 sm:flex-col sm:items-end">
                  {isChat && evId && onJumpToMessage ? (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => onJumpToMessage(evId)}
                      aria-label={t("openMessageContext")}
                      title={t("openMessage").replace("↗ ", "")}
                    >
                      {t("openMessage")}
                    </Button>
                  ) : null}
                  {isChat && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => onReply(ev)}
                      aria-label={`Reply to ${getDisplayName(ev.by || "") || "message"}`}
                      title={t("reply")}
                    >
                      {t("replyTo")}
                    </Button>
                  )}
                  {evId && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      onClick={() => {
                        void copyWithFeedback(evId, {
                          successMessage: t("common:copied"),
                          errorMessage: t("common:copyFailed"),
                        });
                      }}
                      aria-label={t("copyEventId")}
                      title={t("copyEventId")}
                    >
                      {t("copyId")}
                    </Button>
                  )}
                </div>
              </div>
            </article>
          );
        })}

        {!busy && !error && results.length === 0 && (
          <div className="flex flex-col items-center gap-2 px-4 py-10 text-center">
            <SearchIcon size={22} className="text-[var(--color-text-tertiary)]" />
            <p className="text-sm text-[var(--color-text-secondary)]">
              {t(searchedQuery === null ? "searchStart" : "noResults")}
            </p>
            <p className="text-xs text-[var(--color-text-tertiary)]">
              {t(searchedQuery === null ? "searchStartHint" : "noResultsHint")}
            </p>
          </div>
        )}
      </div>
    </ModalFrame>
  );
}
