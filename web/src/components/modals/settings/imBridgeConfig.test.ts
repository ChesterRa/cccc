import { describe, expect, it } from "vite-plus/test";

import { canStartIMBridge } from "./imBridgeConfig";

describe("canStartIMBridge", () => {
  it("requires Weixin login before starting", () => {
    expect(canStartIMBridge("weixin", false)).toBe(false);
    expect(canStartIMBridge("weixin", true)).toBe(true);
  });

  it("does not apply the Weixin login gate to other platforms", () => {
    expect(canStartIMBridge("dingtalk", false)).toBe(true);
    expect(canStartIMBridge("telegram", false)).toBe(true);
  });
});
