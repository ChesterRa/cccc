import { describe, expect, it } from "vitest";

import type { Actor } from "../types";
import {
  deriveRuntimeStatusFromActors,
  EMPTY_CHAT_BUCKET,
  INITIAL_LEDGER_TAIL_LIMIT,
  mergeActorUnreadCounts,
  reuseEqualActors,
  shouldDeferInitialTailRefresh,
} from "./groupStoreCore";

describe("actor refresh stability", () => {
  it("keeps initial ledger tail lightweight for group switching", () => {
    expect(INITIAL_LEDGER_TAIL_LIMIT).toBe(120);
  });

  it("defers initial tail refresh when cached events can render immediately", () => {
    expect(shouldDeferInitialTailRefresh({
      ...EMPTY_CHAT_BUCKET,
      events: [
        {
          id: "ev-1",
          kind: "chat.message",
          group_id: "g_cached",
          data: { text: "cached" },
        },
      ],
    })).toBe(true);

    expect(shouldDeferInitialTailRefresh(EMPTY_CHAT_BUCKET)).toBe(false);
  });

  it("reuses the previous actor array when refresh payloads are unchanged", () => {
    const previous: Actor[] = [
      {
        id: "claude-1",
        role: "peer",
        runtime: "claude",
        runner: "pty",
        running: true,
        unread_count: 2,
      },
    ];
    const refreshed: Actor[] = [
      {
        id: "claude-1",
        role: "peer",
        runtime: "claude",
        runner: "pty",
        running: true,
        unread_count: 2,
      },
    ];

    expect(reuseEqualActors(refreshed, previous)).toBe(previous);
  });

  it("preserves array identity when readonly refresh only reuses existing unread counts", () => {
    const previous: Actor[] = [
      {
        id: "claude-1",
        role: "peer",
        runtime: "claude",
        runner: "pty",
        running: true,
        unread_count: 2,
      },
    ];
    const readonlyRefresh: Actor[] = [
      {
        id: "claude-1",
        role: "peer",
        runtime: "claude",
        runner: "pty",
        running: true,
      },
    ];

    expect(mergeActorUnreadCounts(readonlyRefresh, previous)).toBe(previous);
  });

  it("keeps fallback lifecycle when actors refresh has no running actors", () => {
    const actors: Actor[] = [
      {
        id: "claude-1",
        role: "foreman",
        runtime: "codex",
        runner: "pty",
        running: false,
      },
    ];

    expect(deriveRuntimeStatusFromActors(actors, { lifecycle_state: "active" }).lifecycle_state).toBe("active");
    expect(deriveRuntimeStatusFromActors(actors, { lifecycle_state: "stopped" }).lifecycle_state).toBe("stopped");
  });
});
