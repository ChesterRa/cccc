import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { fetchTerminalHistory } from "../../services/api";
import { ModalFrame } from "../modals/ModalFrame";
import { useModalA11y } from "../../hooks/useModalA11y";
import {
  EMPTY_TERMINAL_HISTORY,
  appendOlderPage,
  canLoadOlder,
  historyText,
  isEmptyHistory,
  type TerminalHistoryState,
} from "./terminalHistoryPager";

const PAGE_LIMIT_BYTES = 64_000;
// Start fetching before the user actually hits the top, so the next page is
// usually already in place by the time they get there.
const LOAD_OLDER_THRESHOLD_PX = 160;

interface TerminalHistoryPanelProps {
  groupId: string;
  actorId: string;
  actorTitle: string;
  isDark: boolean;
  onClose: () => void;
}

export function TerminalHistoryPanel({
  groupId,
  actorId,
  actorTitle,
  isDark,
  onClose,
}: TerminalHistoryPanelProps) {
  const { t } = useTranslation("actors");
  const { modalRef } = useModalA11y(true, onClose);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [state, setState] = useState<TerminalHistoryState>(EMPTY_TERMINAL_HISTORY);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Scroll anchoring: prepending older text grows the scroll container upwards,
  // which would otherwise yank the reader away from the line they were on.
  const anchorRef = useRef<number | null>(null);
  const loadingRef = useRef(false);
  const stateRef = useRef(state);
  stateRef.current = state;

  const loadOlder = useCallback(async () => {
    const current = stateRef.current;
    if (loadingRef.current || !canLoadOlder(current, false)) return;
    loadingRef.current = true;
    setLoading(true);
    setError(null);
    const element = scrollRef.current;
    anchorRef.current = element ? element.scrollHeight - element.scrollTop : null;
    try {
      const response = await fetchTerminalHistory(groupId, actorId, {
        before: current.nextBefore,
        renderBefore: current.endCursor,
        limitBytes: PAGE_LIMIT_BYTES,
        stripAnsi: true,
        compact: false,
      });
      if (response.ok) {
        setState((previous) => appendOlderPage(previous, response.result));
      } else {
        setError(response.error?.message || t("historyLoadFailed"));
      }
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : t("historyLoadFailed"));
    } finally {
      loadingRef.current = false;
      setLoading(false);
    }
  }, [actorId, groupId, t]);

  useEffect(() => {
    void loadOlder();
    // Only the initial page; later pages come from the scroll handler.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (!element) return;
    const anchor = anchorRef.current;
    anchorRef.current = null;
    if (anchor === null) {
      // First page: the newest bytes are at the bottom, which is where the
      // reader expects to start.
      element.scrollTop = element.scrollHeight;
      return;
    }
    element.scrollTop = element.scrollHeight - anchor;
  }, [state.pages]);

  const handleScroll = useCallback(() => {
    const element = scrollRef.current;
    if (error || !element || element.scrollTop > LOAD_OLDER_THRESHOLD_PX) return;
    if (element.clientHeight > 0 && element.scrollHeight <= element.clientHeight) return;
    void loadOlder();
  }, [error, loadOlder]);

  const text = historyText(state);
  const empty = isEmptyHistory(state, loading);

  return (
    <ModalFrame
      modalRef={modalRef}
      isDark={isDark}
      onClose={onClose}
      titleId="terminal-history-title"
      closeAriaLabel={t("common:close")}
      panelClassName="h-full w-full sm:h-[85vh] sm:max-w-3xl"
      title={
        <div className="min-w-0">
          <div className="truncate text-sm font-semibold text-[var(--color-text-primary)]">
            {t("terminalHistory")}
          </div>
          <div className="truncate text-xs text-[var(--color-text-tertiary)]">{actorTitle}</div>
        </div>
      }
    >
      <div
        ref={scrollRef}
        onScroll={handleScroll}
        className="min-h-0 flex-1 overflow-auto overscroll-contain px-4 py-3 sm:px-6"
      >
        {state.expired ? (
          <div className="mb-3 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-2 text-xs text-[var(--color-text-tertiary)]">
            {t("historyTruncated")}
          </div>
        ) : null}

        {loading && state.pages.length === 0 ? (
          <div className="py-8 text-center text-sm text-[var(--color-text-tertiary)]">
            {t("historyLoading")}
          </div>
        ) : null}

        {state.hasMore && !error ? (
          <button
            type="button"
            disabled={loading}
            onClick={() => void loadOlder()}
            className="mb-3 rounded-lg border border-[var(--glass-border-subtle)] px-3 py-1.5 text-xs text-[var(--color-text-primary)] disabled:opacity-50"
          >
            {t("historyLoadOlder")}
          </button>
        ) : null}

        {!state.hasMore && state.pages.length > 0 ? (
          <div className="pb-2 text-center text-xs text-[var(--color-text-tertiary)]">
            {t("historyStart")}
          </div>
        ) : null}

        {loading && state.pages.length > 0 ? (
          <div className="pb-2 text-center text-xs text-[var(--color-text-tertiary)]">
            {t("historyLoading")}
          </div>
        ) : null}

        {empty && !error ? (
          <div className="py-8 text-center text-sm text-[var(--color-text-tertiary)]">
            {t("historyEmpty")}
          </div>
        ) : null}

        {text ? (
          <pre className="whitespace-pre-wrap break-words font-mono text-[12px] leading-[1.5] text-[var(--color-text-primary)]">
            {text}
          </pre>
        ) : null}

        {error ? (
          <div className="py-4 text-center text-sm text-[var(--color-text-tertiary)]">
            <div className="mb-2">{error}</div>
            <button
              type="button"
              onClick={() => void loadOlder()}
              className="rounded-lg border border-[var(--glass-border-subtle)] px-3 py-1.5 text-xs text-[var(--color-text-primary)]"
            >
              {t("historyRetry")}
            </button>
          </div>
        ) : null}
      </div>
    </ModalFrame>
  );
}
