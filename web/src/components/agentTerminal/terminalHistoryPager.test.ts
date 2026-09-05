import { describe, expect, it } from "vite-plus/test";

import {
  EMPTY_TERMINAL_HISTORY,
  appendOlderPage,
  canLoadOlder,
  historyText,
  isEmptyHistory,
} from "./terminalHistoryPager";
import type { TerminalHistoryPage } from "../../services/api";

function page(overrides: Partial<TerminalHistoryPage>): TerminalHistoryPage {
  return {
    text: "chunk",
    hint: "",
    start_cursor: 100,
    end_cursor: 200,
    has_more: true,
    cursor_expired: false,
    ...overrides,
  };
}

describe("terminalHistoryPager", () => {
  it("replaces the cumulative rendering and keeps the snapshot end fixed", () => {
    const first = appendOlderPage(
      EMPTY_TERMINAL_HISTORY,
      page({ text: "newer", start_cursor: 80 }),
    );
    expect(first.nextBefore).toBe(80);
    expect(historyText(first)).toBe("newer");

    const second = appendOlderPage(first, page({ text: "older\nnewer", start_cursor: 40 }));
    expect(second.nextBefore).toBe(40);
    expect(historyText(second)).toBe("older\nnewer");
    expect(second.endCursor).toBe(first.endCursor);
    expect(second.pages).toHaveLength(1);
    expect(canLoadOlder(second, false)).toBe(true);
  });

  it("stops when the server reports no more history", () => {
    const state = appendOlderPage(
      EMPTY_TERMINAL_HISTORY,
      page({ text: "all", start_cursor: 0, has_more: false }),
    );
    expect(state.hasMore).toBe(false);
    expect(state.nextBefore).toBeNull();
    expect(canLoadOlder(state, false)).toBe(false);
    expect(historyText(state)).toBe("all");
  });

  it("stops when the cursor stops advancing so the same range is never refetched", () => {
    const first = appendOlderPage(EMPTY_TERMINAL_HISTORY, page({ start_cursor: 50 }));
    // Server clamped at the start of the backlog and returned the same cursor.
    const stuck = appendOlderPage(first, page({ text: "again", start_cursor: 50 }));
    expect(stuck.hasMore).toBe(false);
    expect(stuck.nextBefore).toBeNull();
  });

  it("keeps paging through a page that rendered to nothing while the server has more", () => {
    const first = appendOlderPage(
      EMPTY_TERMINAL_HISTORY,
      page({ text: "visible", start_cursor: 80 }),
    );
    // Control sequences only, or output wiped by a clear screen: nothing to
    // show, but the cursor moved and the server says older bytes exist.
    const blank = appendOlderPage(first, page({ text: "", start_cursor: 40 }));
    expect(blank.hasMore).toBe(true);
    expect(blank.nextBefore).toBe(40);
    expect(historyText(blank)).toBe("");

    const older = appendOlderPage(blank, page({ text: "older", start_cursor: 0, has_more: false }));
    expect(historyText(older)).toBe("older");
    expect(older.hasMore).toBe(false);
  });

  it("keeps an expired-cursor warning sticky across later pages", () => {
    const first = appendOlderPage(
      EMPTY_TERMINAL_HISTORY,
      page({ start_cursor: 90, cursor_expired: true }),
    );
    expect(first.expired).toBe(true);
    const second = appendOlderPage(first, page({ start_cursor: 60 }));
    expect(second.expired).toBe(true);
  });

  it("retains visible text and stops when retention overtakes the pinned snapshot", () => {
    const first = appendOlderPage(
      EMPTY_TERMINAL_HISTORY,
      page({ text: "already visible", start_cursor: 80, end_cursor: 200 }),
    );
    const expired = appendOlderPage(
      first,
      page({ text: "", start_cursor: 200, end_cursor: 200, cursor_expired: true, has_more: false }),
    );
    expect(historyText(expired)).toBe("already visible");
    expect(expired.endCursor).toBe(200);
    expect(expired.expired).toBe(true);
    expect(canLoadOlder(expired, false)).toBe(false);
  });

  it("never blocks loading while a request is already in flight", () => {
    expect(canLoadOlder(EMPTY_TERMINAL_HISTORY, true)).toBe(false);
    expect(canLoadOlder(EMPTY_TERMINAL_HISTORY, false)).toBe(true);
  });

  it("reports an empty backlog only once loading has finished", () => {
    expect(isEmptyHistory(EMPTY_TERMINAL_HISTORY, true)).toBe(false);
    expect(isEmptyHistory(EMPTY_TERMINAL_HISTORY, false)).toBe(true);
    const loaded = appendOlderPage(EMPTY_TERMINAL_HISTORY, page({}));
    expect(isEmptyHistory(loaded, false)).toBe(false);
  });
});
