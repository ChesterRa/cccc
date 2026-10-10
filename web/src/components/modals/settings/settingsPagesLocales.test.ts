import { describe, expect, it } from "vite-plus/test";

import enCommon from "../../../i18n/locales/en/common.json";
import enSettings from "../../../i18n/locales/en/settings.json";
import jaCommon from "../../../i18n/locales/ja/common.json";
import jaSettings from "../../../i18n/locales/ja/settings.json";
import zhCommon from "../../../i18n/locales/zh/common.json";
import zhSettings from "../../../i18n/locales/zh/settings.json";

// Copy the settings pages render with an English inline fallback. A missing key
// silently shows that English text in every other language.
const copy = (
  common: Record<string, unknown>,
  settings: { guidance: Record<string, unknown> },
) => ({
  chooseFile: common.chooseFile,
  noFileChosen: common.noFileChosen,
  sharedScopes: settings.guidance.commonAndRolesTitle,
});

describe("settings page copy with English fallbacks", () => {
  it("is defined in every locale and translated outside English", () => {
    const en = copy(enCommon, enSettings);
    expect(en).toEqual({
      chooseFile: "Choose File",
      noFileChosen: "No file chosen",
      sharedScopes: "Shared Scopes",
    });
    for (const translated of [copy(zhCommon, zhSettings), copy(jaCommon, jaSettings)]) {
      for (const key of Object.keys(en) as (keyof typeof en)[]) {
        expect(typeof translated[key]).toBe("string");
        expect(translated[key]).not.toBe(en[key]);
      }
    }
  });
});
