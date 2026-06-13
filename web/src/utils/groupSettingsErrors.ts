import type { TFunction } from "i18next";

type UnknownRecord = Record<string, unknown>;

type SettingsErrorShape = {
  code?: unknown;
  message?: unknown;
  details?: unknown;
};

function asString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

function asRecord(value: unknown): UnknownRecord | null {
  return value && typeof value === "object" ? (value as UnknownRecord) : null;
}

export function formatGroupSettingsUpdateError(
  t: TFunction,
  error: SettingsErrorShape | null | undefined,
): string {
  const code = asString(error?.code).trim();
  const message = asString(error?.message).trim();
  const details = asRecord(error?.details);

  if (code === "permission_denied") {
    return t("modals:context.settingsPermissionDenied", "You do not have permission to update these settings.");
  }
  if (code === "group_not_found") {
    return t("modals:context.settingsGroupNotFound", "Working group not found.");
  }
  if (code === "missing_group_id") {
    return t("modals:context.settingsMissingGroup", "Missing working group.");
  }
  if (code === "invalid_patch") {
    return t("modals:context.settingsInvalidPatch", "Invalid settings update payload.");
  }

  if (code === "group_settings_update_failed") {
    const cause = asString(details?.cause).trim();
    return message
      ? t("modals:context.failedToUpdateSettingsWithCause", {
          defaultValue: "Failed to update group settings: {{cause}}",
          cause: cause || message,
        })
      : t("modals:context.failedToUpdateSettings", "Failed to update group settings.");
  }

  if (code && message) return `${code}: ${message}`;
  return message || t("modals:context.failedToUpdateSettings", "Failed to update group settings.");
}
