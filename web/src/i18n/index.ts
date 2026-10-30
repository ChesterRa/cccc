import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { createLocaleBackend, type LocaleModuleLoaders } from "./localeBackend";
import { SUPPORTED_LANGUAGES, normalizeLanguageCode } from "./languages";

const localeLoaders = import.meta.glob("./locales/*/*.json") as LocaleModuleLoaders;

// Register before init: the first detected language must be on the document
// before main.tsx renders, as well as after later preference changes.
i18n.on("languageChanged", (lng) => {
  const normalized = normalizeLanguageCode(lng);
  if (typeof document !== "undefined") {
    document.documentElement.lang = normalizeLanguageCode(i18n.resolvedLanguage ?? normalized);
  }
  if (lng !== normalized) {
    void i18n.changeLanguage(normalized);
  }
});

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
      order: ["querystring", "localStorage", "navigator"],
      lookupQuerystring: "lang",
      lookupLocalStorage: "cccc-language",
      caches: ["localStorage"],
    },
    supportedLngs: SUPPORTED_LANGUAGES,
    nonExplicitSupportedLngs: true,
    cleanCode: true,
  });

export default i18n;
