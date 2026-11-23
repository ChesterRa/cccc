import i18n from "../i18n";

type DaemonErrorShape = { code?: unknown; message?: unknown };

function asString(value: unknown): string {
  return typeof value === "string" ? value : value == null ? "" : String(value);
}

/**
 * Formats daemon API errors for user-facing banners, mapping common actionable
 * codes to localized guidance with the original code in parentheses.
 */
export function formatDaemonError(error: DaemonErrorShape | null | undefined): string {
  const code = asString(error?.code).trim();
  const message = asString(error?.message).trim();
  switch (code) {
    case "delivery_in_progress":
      return i18n.t("common:daemonErrors.delivery_in_progress", { code });
    case "delivery_ambiguous":
      return i18n.t("common:daemonErrors.delivery_ambiguous", { code });
    case "already_delivered":
      return i18n.t("common:daemonErrors.already_delivered", { code });
    case "shutdown_in_progress":
      return i18n.t("common:daemonErrors.shutdown_in_progress", { code });
    default:
      if (code && message) return `${code}: ${message}`;
      return message || code;
  }
}
