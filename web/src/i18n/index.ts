import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { SUPPORTED_LANGUAGES, normalizeLanguageCode } from "./languages";

// English
import commonEn from "./locales/en/common.json";
import layoutEn from "./locales/en/layout.json";
import chatEn from "./locales/en/chat.json";
import modalsEn from "./locales/en/modals.json";
import settingsEn from "./locales/en/settings.json";
import actorsEn from "./locales/en/actors.json";

// Chinese
import commonZh from "./locales/zh/common.json";
import layoutZh from "./locales/zh/layout.json";
import chatZh from "./locales/zh/chat.json";
import modalsZh from "./locales/zh/modals.json";
import settingsZh from "./locales/zh/settings.json";
import actorsZh from "./locales/zh/actors.json";

// Japanese
import commonJa from "./locales/ja/common.json";
import layoutJa from "./locales/ja/layout.json";
import chatJa from "./locales/ja/chat.json";
import modalsJa from "./locales/ja/modals.json";
import settingsJa from "./locales/ja/settings.json";
import actorsJa from "./locales/ja/actors.json";

const resources = {
  en: {
    common: commonEn,
    layout: layoutEn,
    chat: chatEn,
    modals: modalsEn,
    settings: settingsEn,
    actors: actorsEn,
  },
  zh: {
    common: commonZh,
    layout: layoutZh,
    chat: chatZh,
    modals: modalsZh,
    settings: settingsZh,
    actors: actorsZh,
  },
  ja: {
    common: commonJa,
    layout: layoutJa,
    chat: chatJa,
    modals: modalsJa,
    settings: settingsJa,
    actors: actorsJa,
  },
};

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: "en",
    defaultNS: "common",
    ns: ["common", "layout", "chat", "modals", "settings", "actors"],
    interpolation: {
      escapeValue: false, // React already escapes
    },
    detection: {
      order: ["localStorage", "navigator"],
      lookupLocalStorage: "cccc-language",
      caches: ["localStorage"],
    },
    supportedLngs: SUPPORTED_LANGUAGES,
    nonExplicitSupportedLngs: true,
    cleanCode: true,
  });

i18n.on("languageChanged", (lng) => {
  const normalized = normalizeLanguageCode(lng);
  if (lng !== normalized) {
    void i18n.changeLanguage(normalized);
  }
});

export default i18n;
