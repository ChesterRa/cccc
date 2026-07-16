import { describe, expect, it } from "vite-plus/test";

import { cacheMessageRowHeight, getCachedMessageRowHeight } from "./rowHeightCache";

describe("message row height cache", () => {
  it("keeps measured heights isolated by chat view", () => {
    cacheMessageRowHeight("g-a:live", "event-1", 241.4);
    cacheMessageRowHeight("g-b:live", "event-1", 118.6);

    expect(getCachedMessageRowHeight("g-a:live", "event-1")).toBe(241);
    expect(getCachedMessageRowHeight("g-b:live", "event-1")).toBe(119);
  });

  it("ignores invalid measurements", () => {
    cacheMessageRowHeight("g-invalid:live", "event-1", 0);
    expect(getCachedMessageRowHeight("g-invalid:live", "event-1")).toBeUndefined();
  });
});
