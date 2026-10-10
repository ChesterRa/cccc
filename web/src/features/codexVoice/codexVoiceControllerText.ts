import type { CodexVoiceReadiness } from "../../services/api";
import type { CodexVoiceSessionController } from "./useCodexVoiceSessionController";

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
  if (!readiness || readiness.realtime_credentials_available) return "";
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
