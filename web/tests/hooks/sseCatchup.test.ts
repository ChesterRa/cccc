import { describe, expect, it, vi } from "vite-plus/test";
import { runReconnectCatchup, scheduleContextOverviewCatchup } from "../../src/hooks/sseCatchup";

describe("sseCatchup", () => {
  it("reconnect catch-up refreshes the task-free overview first and unread last", async () => {
    const invalidateContextRead = vi.fn();
    const reconcileLedgerTail = vi.fn().mockResolvedValue(undefined);
    const refreshActors = vi.fn().mockResolvedValue(undefined);
    const fetchContextOverview = vi.fn().mockResolvedValue(undefined);

    await runReconnectCatchup("g-demo", {
      invalidateContextRead,
      reconcileLedgerTail,
      refreshActors,
      fetchContextOverview,
    });

    expect(invalidateContextRead).toHaveBeenCalledWith("g-demo");
    expect(reconcileLedgerTail).toHaveBeenCalledWith("g-demo");
    expect(refreshActors).toHaveBeenNthCalledWith(1, "g-demo", { includeUnread: false });
    expect(fetchContextOverview).toHaveBeenCalledWith("g-demo", { detail: "overview" });
    expect(refreshActors).toHaveBeenNthCalledWith(2, "g-demo", { includeUnread: true });
  });

  it("context sync catch-up clears the old timer and re-schedules an overview refresh", () => {
    const invalidateContextRead = vi.fn();
    const clearTimer = vi.fn();
    const fetchContextOverview = vi.fn();

    let scheduledDelay = -1;
    let scheduledCallback: (() => void) | null = null;
    const nextTimer = scheduleContextOverviewCatchup("g-demo", {
      invalidateContextRead,
      existingTimer: 17,
      clearTimer,
      setTimer: (cb, delayMs) => {
        scheduledCallback = cb;
        scheduledDelay = delayMs;
        return 23;
      },
      fetchContextOverview,
    });

    expect(invalidateContextRead).toHaveBeenCalledWith("g-demo");
    expect(clearTimer).toHaveBeenCalledWith(17);
    expect(scheduledDelay).toBe(150);
    expect(nextTimer).toBe(23);

    scheduledCallback?.();
    expect(fetchContextOverview).toHaveBeenCalledWith("g-demo", { detail: "overview" });
  });
});
