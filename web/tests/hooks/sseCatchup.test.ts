import { describe, expect, it, vi } from "vite-plus/test";
import { runReconnectCatchup, scheduleContextSummaryCatchup } from "../../src/hooks/sseCatchup";

describe("sseCatchup", () => {
  it("reconnect catch-up refreshes summary first and unread last", async () => {
    const invalidateContextRead = vi.fn();
    const reconcileLedgerTail = vi.fn().mockResolvedValue(undefined);
    const refreshActors = vi.fn().mockResolvedValue(undefined);
    const fetchContextSummary = vi.fn().mockResolvedValue(undefined);

    await runReconnectCatchup("g-demo", {
      invalidateContextRead,
      reconcileLedgerTail,
      refreshActors,
      fetchContextSummary,
    });

    expect(invalidateContextRead).toHaveBeenCalledWith("g-demo");
    expect(reconcileLedgerTail).toHaveBeenCalledWith("g-demo");
    expect(refreshActors).toHaveBeenNthCalledWith(1, "g-demo", { includeUnread: false });
    expect(fetchContextSummary).toHaveBeenCalledWith("g-demo", { detail: "summary" });
    expect(refreshActors).toHaveBeenNthCalledWith(2, "g-demo", { includeUnread: true });
  });

  it("context sync catch-up clears the old timer and re-schedules a summary refresh", () => {
    const invalidateContextRead = vi.fn();
    const clearTimer = vi.fn();
    const fetchContextSummary = vi.fn();

    let scheduledDelay = -1;
    let scheduledCallback: (() => void) | null = null;
    const nextTimer = scheduleContextSummaryCatchup("g-demo", {
      invalidateContextRead,
      existingTimer: 17,
      clearTimer,
      setTimer: (cb, delayMs) => {
        scheduledCallback = cb;
        scheduledDelay = delayMs;
        return 23;
      },
      fetchContextSummary,
    });

    expect(invalidateContextRead).toHaveBeenCalledWith("g-demo");
    expect(clearTimer).toHaveBeenCalledWith(17);
    expect(scheduledDelay).toBe(150);
    expect(nextTimer).toBe(23);

    scheduledCallback?.();
    expect(fetchContextSummary).toHaveBeenCalledWith("g-demo", { detail: "summary" });
  });
});
