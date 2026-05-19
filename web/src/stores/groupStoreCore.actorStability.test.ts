import { describe, expect, it } from "vitest";

import type { Actor } from "../types";
import {
  mergeActorUnreadCounts,
  reuseEqualActors,
} from "./groupStoreCore";

describe("actor refresh stability", () => {
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
});
