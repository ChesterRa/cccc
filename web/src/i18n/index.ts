import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";

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
  });

export default i18n;
