import type { TerminalHistoryPage } from "../../services/api";

// Backward pager over the server-side PTY backlog.
//
// This exists because a runtime that repaints in place — Claude draws every
// frame with absolute cursor moves and emits no line feeds — never pushes a
// line off the top of the screen, so xterm's scrollback stays empty and there
// is nothing for the terminal itself to scroll back to. The bytes are on the
// server either way, so history is read from there instead.

export interface TerminalHistoryState {
  /** Latest cumulative rendering; older renders are replaced, never concatenated. */
  pages: TerminalHistoryPage[];
  endCursor: number | null;
  /** `before` value for the next request; null once the start is reached. */
  nextBefore: number | null;
  hasMore: boolean;
  /** The backlog rotated past the cursor we asked for; older bytes are gone. */
  expired: boolean;
}

export const EMPTY_TERMINAL_HISTORY: TerminalHistoryState = {
  pages: [],
  endCursor: null,
  nextBefore: null,
  hasMore: true,
  expired: false,
};

/**
 * Fold one older page onto the state.
 *
 * Whether more history exists is the server's call, made through `has_more`
 * and a `start_cursor` that keeps moving backwards. The rendered text says
 * nothing about it: a page of pure control sequences, or one wiped by a clear
 * screen, strips to an empty string while older, visible output still sits
 * behind it. Only a cursor that stops moving ends paging early, because the
 * server clamps `start_cursor` at the beginning of the backlog and the same
 * range would otherwise be requested forever.
 */
export function appendOlderPage(
  state: TerminalHistoryState,
  page: TerminalHistoryPage,
): TerminalHistoryState {
  const advanced = state.nextBefore === null || page.start_cursor < state.nextBefore;
  const exhausted = !page.has_more || !advanced;
  return {
    pages: advanced ? [page] : state.pages,
    endCursor: state.endCursor ?? page.end_cursor,
    nextBefore: exhausted ? null : page.start_cursor,
    hasMore: !exhausted,
    expired: state.expired || !!page.cursor_expired,
  };
}

export function canLoadOlder(state: TerminalHistoryState, loading: boolean): boolean {
  return !loading && state.hasMore;
}

export function historyText(state: TerminalHistoryState): string {
  return state.pages[0]?.text ?? "";
}

/** True once a load has completed and produced nothing to show. */
export function isEmptyHistory(state: TerminalHistoryState, loading: boolean): boolean {
  return !loading && !historyText(state);
}
