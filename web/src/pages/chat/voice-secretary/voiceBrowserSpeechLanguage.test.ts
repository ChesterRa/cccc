import { describe, expect, it } from "vite-plus/test";
import { resolveBrowserSpeechLanguage } from "./voiceBrowserSpeechSupport";

describe("Browser ASR recognition language", () => {
  it.each([
    ["ja", "ja-JP"],
    ["en", "en-US"],
    ["zh", "zh-CN"],
    ["ko", "ko-KR"],
    ["fr", "fr-FR"],
    ["de", "de-DE"],
    ["es", "es-ES"],
  ])("expands system language %s for recognition", (systemLanguage, expected) => {
    expect(resolveBrowserSpeechLanguage("auto", systemLanguage)).toBe(expected);
  });

  it.each(["en-GB", "en-AU", "zh-TW", "zh-HK", "pt-PT", "es-MX", "ja-JP"])(
    "preserves the explicitly selected locale %s",
    (language) => {
      expect(resolveBrowserSpeechLanguage(language, "ja")).toBe(language);
    },
  );

  it("uses the script when inferring the missing region", () => {
    expect(resolveBrowserSpeechLanguage("auto", "zh-Hant")).toBe("zh-Hant-TW");
    expect(resolveBrowserSpeechLanguage("auto", "zh-Hans")).toBe("zh-Hans-CN");
  });

  it("also expands an explicitly configured bare language", () => {
    expect(resolveBrowserSpeechLanguage("ja", "en-US")).toBe("ja-JP");
  });

  it("resolves the browser's mixed-language alias and absent preferences", () => {
    expect(resolveBrowserSpeechLanguage("mixed", "ja")).toBe("ja-JP");
    expect(resolveBrowserSpeechLanguage("", "ja")).toBe("ja-JP");
    expect(resolveBrowserSpeechLanguage("auto", "")).toBe("en-US");
  });

  it("does not silently select a different language for an invalid preference", () => {
    expect(resolveBrowserSpeechLanguage("invalid_tag", "ja")).toBe("invalid_tag");
  });
});
