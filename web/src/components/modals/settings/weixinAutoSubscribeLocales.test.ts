import { describe, expect, it } from "vite-plus/test";

import en from "../../../i18n/locales/en/settings.json";
import ja from "../../../i18n/locales/ja/settings.json";
import zh from "../../../i18n/locales/zh/settings.json";

const keys = [
  "weixinSubscribeNeedsConfigBody",
  "weixinSubscribeNeedsRunningBody",
  "weixinSubscribeNextBody",
  "weixinSubscribeBoundBody",
  "weixinSetupStep1",
  "weixinSetupStep2",
  "weixinSetupStep3",
] as const;

describe("Weixin automatic subscription copy", () => {
  it("does not instruct QR-login users to send subscribe", () => {
    for (const locale of [en, ja, zh]) {
      for (const key of keys) {
        expect(locale.imBridge[key]).not.toContain("/subscribe");
      }
    }
  });
});
