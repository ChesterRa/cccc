import type { CodexVoiceReadiness } from "../../services/api";

export function codexVoiceReadinessProblem(
  t: (key: string, options?: Record<string, unknown>) => string,
  readiness: CodexVoiceReadiness | null,
): string {
  if (!readiness) return "";
  if (!readiness.analyst_runtime_available) {
    return readiness.analyst_runtime_setup_required
      ? t("codexVoiceAntigravityAcpSetupRequired")
      : t("codexVoiceAnalystRuntimeMissing", { runtime: readiness.analyst_runtime });
  }
  return readiness.realtime_credentials_available ? "" : t("codexVoiceCodexLoginRequired");
}

export function codexVoiceErrorText(
  t: (key: string, options?: Record<string, unknown>) => string,
  code: string,
  providerCode?: string,
) {
  const normalized = String(code || "unknown")
    .trim()
    .toLowerCase();
  if (normalized === "provider_error" && providerCode) {
    return t("codexVoiceErrors.provider_error_with_code", { code: providerCode });
  }
  return t(`codexVoiceErrors.${normalized}`, { defaultValue: t("codexVoiceErrors.unknown") });
}

export function codexVoiceWarningText(
  t: (key: string, options?: Record<string, unknown>) => string,
  code: string,
) {
  const normalized = String(code || "unknown")
    .trim()
    .toLowerCase();
  return t(`codexVoiceWarnings.${normalized}`, { defaultValue: t("codexVoiceWarnings.unknown") });
}

export function tailText(value: string, maxChars: number): string {
  return value.length > maxChars ? value.slice(value.length - maxChars) : value;
}
