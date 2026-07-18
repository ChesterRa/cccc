import { describe, expect, it } from "vite-plus/test";

import enChat from "../../src/i18n/locales/en/chat.json";
import jaChat from "../../src/i18n/locales/ja/chat.json";
import zhChat from "../../src/i18n/locales/zh/chat.json";

describe("ProjectedBrowserSurfacePanel i18n", () => {
  it("defines Xvfb isolation labels in every supported locale", () => {
    const requiredKeys = [
      "presentationBrowserViewerIsolationXvfb",
      "presentationBrowserViewerTooltipIsolationXvfb",
    ] as const;

    for (const locale of [enChat, jaChat, zhChat]) {
      for (const key of requiredKeys) {
        expect(locale[key]).toBeTruthy();
      }
    }
  });
});
