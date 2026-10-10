import type { SecretaryReadiness } from "../../services/api/voiceSecretary";

const READINESS_MESSAGES = {
  not_configured: {
    titleKey: "voiceSettings.readiness.notConfigured",
    hintKey: "voiceSettings.readiness.configureHint",
    action: "settings",
  },
  invalid_configuration: {
    titleKey: "voiceSettings.readiness.invalidConfiguration",
    hintKey: "voiceSettings.readiness.configurationHint",
    action: "settings",
  },
  owner_unavailable: {
    titleKey: "voiceSettings.readiness.ownerUnavailable",
    hintKey: "voiceSettings.readiness.ownerHint",
    action: "execution",
  },
} as const;

/** Unknown/loading observations do not invent a service failure. */
export function secretaryReadinessIssue(state: Partial<SecretaryReadiness> | null | undefined) {
  const code = state?.readiness_code;
  if (state?.ready !== false || !code) return null;
  return {
    code,
    ...READINESS_MESSAGES[code],
    detail: code === "not_configured" ? "" : state.readiness_error || "",
  };
}
