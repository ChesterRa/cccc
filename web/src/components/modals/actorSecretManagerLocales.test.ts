import { describe, expect, it } from "vite-plus/test";

import en from "../../i18n/locales/en/actors.json";
import ja from "../../i18n/locales/ja/actors.json";
import zh from "../../i18n/locales/zh/actors.json";

const REQUIRED_KEYS = [
  "title",
  "configured",
  "noVariables",
  "loading",
  "addVariable",
  "batchPaste",
  "batchHint",
  "batchPlaceholder",
  "batchApply",
  "variableName",
  "value",
  "newValue",
  "invalidKey",
  "update",
  "remove",
  "undo",
  "pendingChanges",
  "pendingAdd",
  "pendingUpdate",
  "pendingRemove",
  "maskedValue",
  "showValue",
  "hideValue",
  "clearAllTitle",
  "clearAllHint",
  "clearAllAction",
  "clearAllPending",
] as const;

describe("actor secret manager locales", () => {
  it.each([
    ["en", en, "Environment variables"],
    ["zh", zh, "环境变量"],
    ["ja", ja, "環境変数"],
  ])("names the actor section after environment variables in %s", (_language, locale, expected) => {
    expect(locale.secretsSection).toBe(expected);
  });

  it.each([
    ["en", en],
    ["zh", zh],
    ["ja", ja],
  ])("defines every secret manager label in %s", (_language, locale) => {
    const labels = locale.secretManager as Record<string, string>;

    for (const key of REQUIRED_KEYS) {
      expect(labels[key], key).toBeTypeOf("string");
      expect(labels[key].trim(), key).not.toBe("");
    }
  });
});
