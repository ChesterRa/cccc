import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import { scheduleDebounced } from "./useDebouncedValue";

describe("scheduleDebounced", () => {
  afterEach(() => vi.useRealTimers());

  it("coalesces rapid value changes", () => {
    vi.useFakeTimers();
    const committed: string[] = [];
    let cancel = scheduleDebounced(() => committed.push("s"), 200);

    cancel();
    cancel = scheduleDebounced(() => committed.push("sk"), 200);
    cancel();
    scheduleDebounced(() => committed.push("skill"), 200);
    expect(committed).toEqual([]);

    vi.advanceTimersByTime(200);
    expect(committed).toEqual(["skill"]);
  });
});
