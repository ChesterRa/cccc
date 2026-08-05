import { describe, expect, it } from "vite-plus/test";

import { CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION } from "../stores/useUIStore";
import { shouldRestoreDetachedScrollSnapshot } from "./useChatTab";

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
