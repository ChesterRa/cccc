import { describe, expect, it } from "vitest";
import { createInstance } from "i18next";
import en from "../../i18n/locales/en/modals.json";
import ja from "../../i18n/locales/ja/modals.json";
import zh from "../../i18n/locales/zh/modals.json";
import enSettings from "../../i18n/locales/en/settings.json";
import jaSettings from "../../i18n/locales/ja/settings.json";
import zhSettings from "../../i18n/locales/zh/settings.json";
import { webApplyErrorText } from "../../components/modals/settings/webAccessReachabilityModel";
import { codexVoiceErrorText, codexVoiceSettingsErrorText } from "./codexVoiceControllerText";
import { failure, failureRecoveryOwners } from "./codexVoiceProtocol";

async function translator(language: string, ns: "modals" | "settings") {
  const i18n = createInstance();
  await i18n.init({
    lng: language,
    resources:
      ns === "modals"
        ? { en: { translation: en }, zh: { translation: zh }, ja: { translation: ja } }
        : {
            en: { translation: enSettings },
            zh: { translation: zhSettings },
            ja: { translation: jaSettings },
          },
  });
  return (key: string, options?: Record<string, unknown>) => String(i18n.t(key, options));
}

describe("Voice restart and cleanup errors", () => {
  it.each(["en", "zh", "ja"])("has its own %s text for each gate code", async (language) => {
    const t = await translator(language, "modals");
    const unknown = t("codexVoiceErrors.unknown");
    const texts = [
      "codex_voice_restarting",
      "codex_voice_recovery_pending",
      "codex_voice_reset_recovery_pending",
      "codex_voice_settings_recovery_pending",
    ].map((code) => codexVoiceErrorText(t, code));
    expect(new Set(texts).size).toBe(4);
    for (const text of texts) {
      expect(text).not.toEqual(unknown);
      expect(text).not.toContain("codexVoiceErrors.");
    }
  });

  it.each(["en", "zh", "ja"])(
    "names the gate codes in %s Analyst settings and keeps host detail otherwise",
    async (language) => {
      const t = await translator(language, "modals");
      expect(codexVoiceSettingsErrorText(t, "codex_voice_recovery_pending", "raw")).toEqual(
        codexVoiceErrorText(t, "codex_voice_recovery_pending"),
      );
      expect(
        codexVoiceSettingsErrorText(t, "codex_voice_recovery_pending", "raw", { retryable: true }),
      ).toEqual(codexVoiceErrorText(t, "codex_voice_recovery_pending"));
      // After the replacement began, the saved settings may already have changed.
      expect(
        codexVoiceSettingsErrorText(t, "codex_voice_recovery_pending", "raw", { retryable: false }),
      ).toEqual(codexVoiceErrorText(t, "codex_voice_settings_recovery_pending"));
      expect(codexVoiceSettingsErrorText(t, "codex_voice_restarting", "raw")).toEqual(
        codexVoiceErrorText(t, "codex_voice_restarting"),
      );
      expect(codexVoiceSettingsErrorText(t, "something_else", "host detail")).toBe("host detail");
    },
  );

  it.each(["en", "zh", "ja"])(
    "tells %s users that Apply left the Web binding unchanged",
    async (language) => {
      const t = await translator(language, "settings");
      const pending = webApplyErrorText(t, {
        code: "codex_voice_recovery_pending",
        message: "Previous Voice resources have not been confirmed cleaned up.",
      });
      const busy = webApplyErrorText(t, { code: "codex_voice_restarting", message: "restarting" });
      expect(pending).toBe(t("webAccess.applyVoiceCleanupPending"));
      expect(busy).toBe(t("webAccess.applyVoiceBusy"));
      expect(pending).not.toContain("webAccess.");
      expect(busy).not.toContain("webAccess.");
      expect(webApplyErrorText(t, { code: "web_apply_failed", message: "host detail" })).toBe(
        "host detail",
      );
      expect(webApplyErrorText(t, null)).toBe(t("webAccess.applyFailed"));
    },
  );
});

describe("Voice start blocked by process records", () => {
  it("keeps the known owners a recovery-pending start names, in a stable order", () => {
    const named = failure("codex_voice_recovery_pending", {
      retryable: true,
      recovery_owners: ["web", "bogus", "daemon", "web"],
    });
    expect(named.recoveryOwners).toEqual(["daemon", "web"]);
    // Other gates carry no owners, and nothing is invented for them.
    expect(failure("codex_voice_recovery_pending", { retryable: true }).recoveryOwners).toEqual([]);
    expect(failure("codex_voice_restarting", { recovery_owners: ["web"] }).recoveryOwners).toEqual(
      [],
    );
    expect(failureRecoveryOwners(named)).toEqual(["daemon", "web"]);
    expect(failureRecoveryOwners(new Error("other"))).toEqual([]);
  });
});
