import type { BackendModule, ReadCallback, ResourceKey } from "i18next";
import { normalizeLanguageCode } from "./languages";

type LocaleModule = { default: ResourceKey };
export type LocaleModuleLoader = () => Promise<LocaleModule>;
export type LocaleModuleLoaders = Record<string, LocaleModuleLoader>;

export function localeModuleKey(language: string, namespace: string): string {
  return `./locales/${normalizeLanguageCode(language)}/${namespace}.json`;
}

export function createLocaleBackend(loaders: LocaleModuleLoaders): BackendModule {
  return {
    type: "backend",
    init() {},
    read(language: string, namespace: string, callback: ReadCallback) {
      const key = localeModuleKey(language, namespace);
      const load = loaders[key];
      if (!load) {
        callback(new Error(`Unsupported locale resource: ${key}`), false);
        return;
      }
      void load().then(
        (module) => callback(null, module.default),
        (error: unknown) =>
          callback(error instanceof Error ? error : new Error(String(error)), false),
      );
    },
  };
}
