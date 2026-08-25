import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { createLocaleBackend, type LocaleModuleLoaders } from "./localeBackend";
import { SUPPORTED_LANGUAGES, normalizeLanguageCode } from "./languages";

const localeLoaders = import.meta.glob("./locales/*/*.json") as LocaleModuleLoaders;

export const i18nReady = i18n
  .use(createLocaleBackend(localeLoaders))
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    fallbackLng: "en",
    defaultNS: "common",
    ns: ["common", "layout", "chat", "modals", "settings", "actors"],
    load: "languageOnly",
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
