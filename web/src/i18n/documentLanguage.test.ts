// @vitest-environment happy-dom
import { afterEach, beforeEach, expect, it, vi } from "vite-plus/test";

let originalLanguages: PropertyDescriptor | undefined;

beforeEach(() => {
  vi.resetModules();
  // i18next is externalized by Vitest; give each import a real, fresh instance
  // rather than accumulating listeners on the shared package singleton.
  vi.doMock("i18next", async (importOriginal) => {
    const actual = await importOriginal<typeof import("i18next")>();
    return { ...actual, default: actual.default.createInstance() };
  });
  window.history.replaceState({}, "", "/ui/");
  window.localStorage.clear();
  document.documentElement.lang = "en";
  originalLanguages = Object.getOwnPropertyDescriptor(window.navigator, "languages");
});

afterEach(() => {
  vi.doUnmock("i18next");
  window.history.replaceState({}, "", "/ui/");
  window.localStorage.clear();
  document.documentElement.lang = "en";
  if (originalLanguages) {
    Object.defineProperty(window.navigator, "languages", originalLanguages);
  } else {
    delete (window.navigator as { languages?: readonly string[] }).languages;
  }
});

async function start() {
  const { default: i18n, i18nReady } = await import("./index");
  await i18nReady;
  return i18n;
}

it.each(["zh", "ja"])("marks the first rendered page with its stored %s language", async (lang) => {
  window.localStorage.setItem("cccc-language", lang);
  const i18n = await start();
  expect(i18n.resolvedLanguage).toBe(lang);
  expect(document.documentElement.lang).toBe(lang);
});

it("uses query language over storage and normalizes a region code", async () => {
  window.localStorage.setItem("cccc-language", "en");
  window.history.replaceState({}, "", "/ui/?lang=zh-CN");
  const i18n = await start();
  expect(i18n.resolvedLanguage).toBe("zh");
  expect(document.documentElement.lang).toBe("zh");
});

it("uses the browser language when no preference was stored", async () => {
  Object.defineProperty(window.navigator, "languages", { configurable: true, value: ["ja-JP"] });
  const i18n = await start();
  expect(i18n.resolvedLanguage).toBe("ja");
  expect(document.documentElement.lang).toBe("ja");
});

it("marks the English fallback when the detected language is unsupported", async () => {
  window.history.replaceState({}, "", "/ui/?lang=fr-FR");
  const i18n = await start();
  expect(i18n.resolvedLanguage).toBe("en");
  expect(document.documentElement.lang).toBe("en");
});

it("tracks later changes and does not react to an independent i18n instance", async () => {
  window.localStorage.setItem("cccc-language", "en");
  const i18n = await start();
  await i18n.changeLanguage("ja");
  expect(document.documentElement.lang).toBe("ja");
  await i18n.changeLanguage("zh-CN");
  expect(document.documentElement.lang).toBe("zh");
  await i18n.changeLanguage("fr-FR");
  expect(document.documentElement.lang).toBe("en");

  const other = i18n.createInstance();
  await other.init({ lng: "ja", resources: { ja: { translation: { hello: "こんにちは" } } } });
  await other.changeLanguage("ja");
  expect(document.documentElement.lang).toBe("en");
});

it("keeps the document in the last language requested while locales load", async () => {
  window.localStorage.setItem("cccc-language", "en");
  const i18n = await start();
  const first = i18n.changeLanguage("zh");
  const last = i18n.changeLanguage("ja");
  await Promise.all([first, last]);
  expect(i18n.resolvedLanguage).toBe("ja");
  expect(document.documentElement.lang).toBe("ja");
});
