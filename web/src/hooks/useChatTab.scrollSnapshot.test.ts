import { describe, expect, it } from "vite-plus/test";

import {
  CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
  type ChatScrollSnapshot,
} from "../stores/useUIStore";
import { shouldRestoreDetachedScrollSnapshot, shouldRestoreScrollSnapshot } from "./useChatTab";

describe("chat scroll snapshot coordinate version", () => {
  it("restores a current signed-offset snapshot", () => {
    expect(
      shouldRestoreDetachedScrollSnapshot(
        {
          coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
          mode: "detached",
          anchorId: "event-1",
          updatedAt: 1_000,
        },
        1_100,
      ),
    ).toBe(true);
  });

  it("rejects legacy snapshots whose clamped offset cannot be recovered", () => {
    expect(
      shouldRestoreDetachedScrollSnapshot(
        { mode: "detached", anchorId: "event-1", updatedAt: 1_000 },
        1_100,
      ),
    ).toBe(false);
  });
});

describe("shouldRestoreScrollSnapshot", () => {
  const atBottom: ChatScrollSnapshot = {
    coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
    mode: "follow",
    anchorId: "",
    offsetPx: 0,
    updatedAt: 1_000,
    viewKey: "g:live:mail:[]",
  };
  const touched = { ...atBottom, visible: [{ id: "event-1", offsetPx: -120 }] };

  it("takes a detached place in any view", () => {
    const detached = { ...atBottom, mode: "detached" as const, anchorId: "event-1" };
    expect(shouldRestoreScrollSnapshot(detached, "g:live:mail:[]", 1_100)).toBe(true);
    expect(shouldRestoreScrollSnapshot(detached, "g:live:all:[]", 1_100)).toBe(true);
  });

  it("takes a place at the bottom only from another view, with a touched message", () => {
    expect(shouldRestoreScrollSnapshot(touched, "g:live:all:[]", 1_100)).toBe(true);
    expect(shouldRestoreScrollSnapshot(touched, "g:live:mail:[]", 1_100)).toBe(false);
    expect(shouldRestoreScrollSnapshot(atBottom, "g:live:all:[]", 1_100)).toBe(false);
    expect(
      shouldRestoreScrollSnapshot({ ...touched, viewKey: undefined }, "g:live:all:[]", 1_100),
    ).toBe(false);
  });

  it("drops a touched place at the bottom that is stale or from older coordinates", () => {
    expect(shouldRestoreScrollSnapshot(touched, "g:live:all:[]", 1_000 + 31 * 60 * 1000)).toBe(
      false,
    );
    expect(
      shouldRestoreScrollSnapshot(
        { ...touched, coordinateVersion: undefined },
        "g:live:all:[]",
        1_100,
      ),
    ).toBe(false);
  });
});
