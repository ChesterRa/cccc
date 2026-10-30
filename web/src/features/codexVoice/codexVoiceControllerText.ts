import type { CodexVoiceReadiness } from "../../services/api";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";
import { formatCodexVoiceName } from "./codexVoicePreferences";

/** Active-call metadata takes precedence over the next-call selection. */
export function codexVoiceDisplayIdentity(
  controller: Pick<CodexVoiceSessionController, "call" | "preferences">,
) {
  const selection = controller.call || controller.preferences;
  return { provider: selection.provider || "codex", voice: formatCodexVoiceName(selection.voice) };
}

/** One call-status projection for the dock, visible header and accessibility announcement. */
export function codexVoiceCallStatus(
  controller: Pick<
    CodexVoiceSessionController,
    "externalCall" | "checking" | "isEngaged" | "readiness" | "phase" | "error" | "refreshError"
  >,
) {
  const blocked =
    !controller.isEngaged &&
    Boolean(
      controller.readiness &&
      (!controller.readiness.realtime_credentials_available ||
        controller.readiness.realtime_runtime_available === false ||
        !controller.readiness.analyst_runtime_available),
    );
  const labelKey = controller.externalCall
    ? "codexVoiceActiveElsewhere"
    : !controller.isEngaged && controller.checking
      ? "codexVoiceChecking"
      : blocked
        ? "codexVoiceCallBlocked"
        : !controller.isEngaged && controller.error
          ? "codexVoicePhase.failed"
          : !controller.isEngaged && controller.refreshError
            ? controller.readiness
              ? "codexVoiceStatusStaleLabel"
              : "codexVoiceStatusUnavailableLabel"
            : !controller.isEngaged && !controller.readiness && controller.phase === "idle"
              ? "codexVoiceChecking"
              : `codexVoicePhase.${controller.phase}`;
  return { blocked, labelKey };
}

export function codexVoiceReadinessProblem(
  t: (key: string, options?: Record<string, unknown>) => string,
  readiness: CodexVoiceReadiness | null,
): string {
  return (
    codexVoiceAnalystReadinessProblem(t, readiness) || codexVoiceCallReadinessProblem(t, readiness)
  );
}

export function codexVoiceAnalystReadinessProblem(
  t: (key: string, options?: Record<string, unknown>) => string,
  readiness: CodexVoiceReadiness | null,
): string {
  if (!readiness || readiness.analyst_runtime_available) return "";
  return readiness.analyst_runtime_setup_required
    ? t("codexVoiceAntigravityAcpSetupRequired")
    : t("codexVoiceAnalystRuntimeMissing", { runtime: readiness.analyst_runtime });
}

export function codexVoiceCallReadinessProblem(
  t: (key: string, options?: Record<string, unknown>) => string,
  readiness: CodexVoiceReadiness | null,
): string {
  if (!readiness) return "";
  if (readiness.realtime_provider === "grok") {
    if (readiness.realtime_runtime_available === false) return t("codexVoiceGrokRuntimeRequired");
    return readiness.realtime_credentials_available ? "" : t("codexVoiceGrokLoginRequired");
  }
  if (readiness.realtime_credentials_available) return "";
  return t("codexVoiceCodexLoginRequired");
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

/** Voice Analyst settings errors; an unknown code shows the host's detail. */
export function codexVoiceSettingsErrorText(
  t: (key: string, options?: Record<string, unknown>) => string,
  code: string,
  detail: string,
  details?: unknown,
) {
  if (code === "codex_voice_call_active") return t("codexVoiceAnalystSettingsCallActive");
  if (code === "codex_voice_settings_busy") return t("codexVoiceAnalystSettingsWorkActive");
  if (code === "codex_voice_settings_unavailable") return t("codexVoiceAnalystSettingsUnavailable");
  if (code === "codex_voice_settings_invalid") {
    return t("codexVoiceAnalystSettingsInvalid", { detail });
  }
  // Not retryable means the replacement had begun, so the saved settings may have changed.
  if (
    code === "codex_voice_recovery_pending" &&
    (details as { retryable?: unknown } | undefined)?.retryable === false
  ) {
    return codexVoiceErrorText(t, "codex_voice_settings_recovery_pending");
  }
  if (code === "codex_voice_restarting" || code === "codex_voice_recovery_pending") {
    return codexVoiceErrorText(t, code);
  }
  return detail;
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
