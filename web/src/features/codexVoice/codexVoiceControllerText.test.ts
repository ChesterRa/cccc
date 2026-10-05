import { describe, expect, it } from "vite-plus/test";
import type { CodexVoiceReadiness } from "../../services/api";
import en from "../../i18n/locales/en/modals.json";
import zh from "../../i18n/locales/zh/modals.json";
import ja from "../../i18n/locales/ja/modals.json";
import { codexVoiceReadinessProblem } from "./codexVoiceControllerText";

const ready: CodexVoiceReadiness = {
  supported_modes: ["assistant", "persona"],
  analyst_runtime: "antigravity",
  analyst_runtime_available: true,
  realtime_credentials_available: true,
};

describe("Voice readiness diagnostics", () => {
  it.each([en, zh, ja])(
    "gives the exact ACP setup command in each supported language",
    (locale) => {
      const text = codexVoiceReadinessProblem(
        (key) => locale[key as keyof typeof locale] as string,
        { ...ready, analyst_runtime_available: false, analyst_runtime_setup_required: true },
      );
      expect(text).toContain("cccc setup --runtime antigravity --runtime-mode acp --login");
      expect(text).not.toBe(locale.codexVoiceAnalystRuntimeMissing);
    },
  );

  it("preserves native runtime diagnostics and credential precedence", () => {
    const t = (key: string, options?: Record<string, unknown>) =>
      `${key}:${String(options?.runtime ?? "")}`;
    expect(
      codexVoiceReadinessProblem(t, {
        ...ready,
        analyst_runtime: "opencode",
        analyst_runtime_available: false,
        realtime_credentials_available: false,
      }),
    ).toBe("codexVoiceAnalystRuntimeMissing:opencode");
    expect(codexVoiceReadinessProblem(t, { ...ready, realtime_credentials_available: false })).toBe(
      "codexVoiceCodexLoginRequired:",
    );
    expect(codexVoiceReadinessProblem(t, ready)).toBe("");
    expect(codexVoiceReadinessProblem(t, null)).toBe("");
  });
});
