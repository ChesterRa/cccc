import { describe, expect, it } from "vite-plus/test";

import { keepsActiveReach, type ReachDraft } from "./webAccessReachabilityModel";

const activeReach: ReachDraft = {
  savedProvider: "reach",
  draftProvider: "reach",
  goal: "public",
  savedMode: "tailnet_only",
  draftMode: "tailnet_only",
  savedHost: "127.0.0.1",
  draftHost: "127.0.0.1",
  savedPort: "8848",
  draftPort: "8848",
  savedPublicUrl: "https://device.example.test",
  draftPublicUrl: "https://device.example.test",
};

describe("Reach configuration ownership", () => {
  it("keeps Reach only for an unchanged Reach draft", () => {
    expect(keepsActiveReach(activeReach)).toBe(true);
    expect(keepsActiveReach({ ...activeReach, draftProvider: "manual" })).toBe(false);
    expect(
      keepsActiveReach({ ...activeReach, draftPublicUrl: "https://manual.example.test" }),
    ).toBe(false);
    expect(keepsActiveReach({ ...activeReach, draftPort: "9000" })).toBe(false);
  });
});
